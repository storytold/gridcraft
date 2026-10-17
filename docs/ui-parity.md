# UI parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-14 · **Change:** minor (punctuation shortcuts follow the keyboard layout's characters) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Interaction, controls and feel, against Excel. Part of [target-app-parity.md](target-app-parity.md).
**UI/UX fidelity: ~55% ready, 50–80 h** (estimated). Ribbon command coverage is measured
separately in [parity-checklist.md](parity-checklist.md) (274 / 290).

Sources: `crates/ui-egui/src` (ribbon, grid, editor, formula bar, dialogs, panes),
`crates/engine/src/cmd` (shortcuts in command specs), the owner's black-box notes
(`plan/excel/observed-ui.md`, local only) and user issues.

## Window and chrome

| Element | Excel | GridCraft | Status |
|---|---|---|---|
| Title bar, Quick Access Toolbar, AutoSave toggle | yes | yes | done; macOS window controls misaligned (#170), Dock icon size (#169) |
| Ribbon: tabs, groups, split buttons, galleries, contextual tabs | yes | yes (Table Design, Chart Design, Format, PivotTable, Header & Footer tabs) | done; missing contextual tabs for pictures, shapes, sparklines, slicers |
| Ribbon collapse, simplified ribbon, customize ribbon/QAT | yes | partial | customization missing (File ▸ Options is the one missing File command) |
| Ribbon: tabs, groups, split buttons, galleries, contextual tabs | yes | yes (Table Design, Chart Design, Format, PivotTable tabs) | done; missing contextual tabs for pictures, shapes, sparklines, header/footer, slicers |
| Ribbon collapse, simplified ribbon, customize ribbon/QAT | yes | partial | customization missing |
| Key tips (Alt / F10 then letters) | yes, every ribbon control | **initial** (#43, 10-10): Alt or F10 shows key tips on Home for alignment, Paste Special and AutoFit plus legacy Alt sequences; other tabs and controls not yet (#33, #49) | 3–5 h |
| Formula bar with Name Box, expand, function hints | yes | yes | done |
| Sheet tabs: rename, reorder, colour, hide, scroll | yes | yes | active tab revealed on navigation and resize (#681); manual wheel/trackpad browsing preserved; rename focus bug on macOS (#145) |
| Status bar: Sum/Count/Average and more, view buttons, zoom | yes | yes | customizable statistics partial |
| Task panes (Format Chart, Comments, Watch, Selection, PivotTable fields) | yes | yes | element-level Format pane for shapes and pictures missing |
| Backstage (File) view: Info, New with templates, Open recent, Export, Options | yes | partial | Options dialog and template gallery missing |
| Dark mode, system appearance | yes | yes | done (10-09); follows the system on first run, title-bar switch and File › Display theme (#194, #239) |
| Backstage (File) view: Info, New with templates, Open recent, Export, Options | yes | yes (#174, 10-11): the File tab opens a full-window view (Esc or the back arrow return) with Home, New (blank + the three samples), Open (recent + Browse; the recent list tells when each workbook was last changed, 10-12), Info (location, properties, Protect Workbook), Save, Save As, Print (page preview, active sheet or workbook, fit to page, Page Setup), Export (PDF, CSV, web page), Close and Options (`file.options`: theme, language, formula bar, AutoSave, calculation) | no Account, Share or template gallery beyond the samples; Options applies at once (no OK/Cancel dialog) and holds only the settings GridCraft has |
| Dark mode, system appearance | yes | yes | done (10-09) |
| Page Layout view (paper view with rulers and header editing) | yes | yes (#329, 10-11): per-sheet view saved in XLSX, pages with margins and gaps on the print pagination, "Click to add data" pages, rulers (inches or centimetres), headers and footers drawn and edited in place with a Header & Footer tab | margins drag on the rulers (#792, with header and footer margins); header/footer pictures, Different First Page / Odd & Even missing (1–3 h) |
| Multiple windows per workbook | yes | no (#162) | see [hardware-parity.md](hardware-parity.md) |

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
| Selection outline glides to a new selection (Excel: "Provide feedback with animation") | done (10-11): 0.12 s, decelerating, with the fill and fill handle; View ▸ Animations (`view.animations`) turns it off. The timing is an estimate, not yet measured against Excel; the system's reduced-motion setting isn't followed yet |
| Fill handle: drag to fill series, double-click to fill down, right-drag menu | fill done (Ctrl+drag swaps copy and series); right-drag menu missing |
| Drag-move and drag-copy cells (border drag, Ctrl/Option, Shift to insert) | done (M3) |
| In-cell editing, Enter/Tab movement, Alt/Option+Enter line breaks | done |
| Point mode (click/drag/arrow references), reference colouring, F4 | done |
| AutoComplete in columns, function autocomplete and argument tips | done; cell helpers clip to the worksheet and hide when the editor scrolls out (#200) |
| Header resize and double-click AutoFit | done (#42, #88 for header dividers) |
| Outline symbols for grouped rows / columns (band beside the headers, brackets, − / +, level buttons 1…n+1) | done (#373): clicks run `data.hideDetail` / `data.showDetail` / `data.showLevels`, frozen panes and right-to-left sheets included; summary rows/columns are always below/right (the "Summary rows below detail" setting isn't modelled), no "Show outline symbols" option yet, and a block of cells still groups rows without the Rows / Columns dialog |
| Reorder selected whole columns by dragging their headers | done (10-11) |
| Freeze panes, split panes | freeze done; split to verify |
| Copy as HTML table for other apps | done (#36, 10-10) |
| Context menu on cells | 16 items (Cut, Copy, Paste, Paste Special, Insert, Delete, Clear Contents, Filter by value, Sort, New Comment, New Note, Format Cells, Pick From Drop-down List, Define Name, Link). Insert Copied Cells (Insert Cut Cells for whole rows/columns) while the clipboard holds a copy (#21). Missing: Insert Cut Cells for a cell block, paste-option icons, Quick Analysis, Get Data from Table, Show Changes, Smart Lookup |
| Move and resize charts, pictures and shapes | charts can't be resized (#190) |
| Context menus on row/column headers, sheet tabs, charts, shapes | headers and tabs done; objects partial |
| Quick Analysis (Ctrl+Q) | missing |
| Smooth scrolling, scrollbars | done; vertical scrollbar added 10-09 (#73); horizontal scrollbar in the tab bar (60/40 split, drag, track click pages) 10-13 (#176, #840); the tabs/scrollbar split can't be dragged yet |
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
| F2, F4, F5, F7, F9 family, Shift+F2/F3/F10/F11, F12 | yes | done; F11 chart sheet, F8 extend mode, F3 Paste Name, F4 Repeat missing |
| Mac-specific (Cmd variants, Control variants, Fn keys) | yes | done where a feature exists (`keymap.rs` picks the Mac variant) |
| Sequential Alt access keys (Alt, H, N, …) on every ribbon tab | yes | 364-sequence keytip table for all tabs: per-control badges, prefix waiting, dropdowns open for editing (type/arrows/Enter), pending bindings for not-yet-built commands; QAT/Backstage excluded |
| Requests | — | #110 (more shortcuts and hot keys), #33 (sequential Alt; initial support landed in #43) |

### Shortcut audit (#110)

Checked against Microsoft's published "Keyboard shortcuts in Excel" (Windows and Mac tables,
2026-10-11). Alt key-tip sequences are tracked separately (#33). Bindings live in
`crates/ui-egui/src/grid.rs` (navigation, editing keys), `ribbon::shortcut` (Ctrl/Cmd letters),
`keymap.rs` (everything added in #110, with a `lookup` table that tests cover) and
`formula_bar.rs` (keys while editing). "Ctrl" means Cmd on a Mac unless the Mac column says
otherwise. Number-format and border keys match the physical key (Ctrl+Shift+4, wherever `$` is).
Punctuation keys go by the character the keyboard layout types, as in Excel: on a Croatian or German
keyboard Ctrl+Shift+, types `;` and inserts the date, Ctrl+Shift+. (`:`) the time, Ctrl++ opens Insert.
A character egui has no key for (`č`, `ö`, `<`) falls back to the key's position.

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
| F1, F3, F4 (repeat), F6 / Shift+F6, F8 / Shift+F8, F11, End mode | same | help, Paste Name, Repeat, pane cycling, extend/add mode, chart sheet | missing (no such feature yet) |
| Ctrl+6, Ctrl+8, Ctrl+Q, Ctrl+Shift+A, Ctrl+A after a function name, Ctrl+Shift+U, Ctrl+Alt+P | same | objects, outline symbols, Quick Analysis, argument names, Function Arguments, formula bar height, tooltips | missing |
| Ctrl+Shift+Page Up / Down, Ctrl+Alt+5, Ctrl+Shift+F1, Ctrl+F5–F10, Alt+F8 / F11 / F12 | same | group sheets, shape cycling, full screen, window keys, macros, VBA, Power Query | missing |
| — | Cmd+= (calculate), Cmd+T (references while editing; F4 works), Shift+F7, Cmd+Shift+L | Mac-only extras | missing (Cmd+= is egui's zoom key; no thesaurus or Modify Cell Style) |

On Windows, AltGr arrives as Ctrl+Alt: a Ctrl+Alt key that also types a character is treated as
text, so AltGr symbols (e.g. `~` on a German layout) never trigger a shortcut.

## Dialogs

Format Cells, Insert Function/Formula Builder, Find and Replace, Go To and Go To Special, Sort,
Name Manager, Data Validation, Paste Special, Conditional Formatting manager, Page Setup, Goal
Seek, Scenario Manager, Data Analysis, Forecast Sheet and others exist. Missing or partial: Options,
Solver, Power Query editor, Insert Object, Equation editor, Protect/Allow Edit Ranges,
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

Key tips 3–5 · remaining shortcuts with their features 2–4 · context menus and Quick Analysis 5–8 · Page Layout view
8–12 · Options dialog and ribbon/QAT customization 6–10 · accessibility 12–18 · selection and
scrollbar bugs 2–4 · pixel/feel pass against observed Excel 7–12.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-14 | minor | Punctuation shortcuts follow the character the keyboard layout types, digits keep their position: on a Croatian or German keyboard Ctrl+Shift+, (`;`) inserts the date instead of decreasing the font size, Ctrl+Shift+. (`:`) the time, Ctrl++ opens Insert, Ctrl+' copies the formula above, and Ctrl+Shift+0 (`=`) unhides columns instead of opening Insert |
| 2026-10-12 | minor | The File view's recent list tells when each workbook was last changed, read off the UI thread |
| 2026-10-13 | minor | Horizontal scrollbar in the sheet tab bar: drag, track click pages, frozen columns excluded from the page (#840) |
| 2026-10-12 | minor | Ctrl+wheel zoom steps along a ladder of percentages about 20% apart that never skips 100%, gliding to each step with animations on, so a notch out undoes a notch in (it drifted: 100 → 122 → 101); Excel steps 15 points, see decisions.md |
| 2026-10-12 | minor | Page Layout view: left/right/top/bottom margins are dragged on the rulers (#792) |
| 2026-10-12 | minor | Alt+Down opens the active filter header's existing popup, with validation priority and mirrored anchoring on right-to-left sheets (part of #441) |
| 2026-10-12 | minor | Paste from the ribbon (button and options menu), the cell and header context menus and the macOS menu bar's Edit menu pastes what is on the system clipboard, as Cmd/Ctrl+V does, instead of only GridCraft's own copy (the web build, which can't read the clipboard on request, still pastes GridCraft's copy). Copy and Cut by keytip (H,C / H,X) publish to the system clipboard like the ribbon buttons. After a control-channel or MCP copy, which leaves the system clipboard alone, Paste pastes that copy until other text is copied. Once a cut has been pasted, Paste (ribbon, menus or Cmd/Ctrl+V) doesn't paste the cut's clipboard text again over the moved cells (Excel pastes a cut once), and the Paste keytip (H,V,P) pastes the system clipboard like the button. Regression test for the Paste button click freeze in 0.5.0 (#273, fixed by #272); the Paste button gets an accessible "Paste" name for screen readers |
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
