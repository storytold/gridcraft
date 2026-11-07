# Developer tab

The Developer tab is a **UI skeleton** for Excel's Developer tab (#2960). It shows Excel's groups
and controls in their observed order, wires the one entry GridCraft already implements (`Macros`),
and renders every other entry as a disabled stub whose tooltip says **"In development — tracked in
#2960"**. It ships no scripting engine and runs no Python, Rust or VBA; Office Scripts are the
implemented scripting engine and live in the **Automate** tab (#3527).

## Observed layout

Observed black-box on this machine, 2026-11-07:

- **Excel version:** Microsoft 365, Excel 16.0 (build 20527), English UI, Developer tab enabled
  (File › Options › Customize Ribbon).
- **Method:** UI Automation walked the selected tab's tree; the window was captured by handle for
  reading only.
- **Screenshot:** `plan/excel/screenshots/excel-developer-1707350.png`, outside the repository
  (`plan/` is gitignored). **No screenshot is committed, linked or published**, and no Microsoft
  icon or artwork is copied: the tab reuses GridCraft's own `Icon`s. Text observations only:

| Group | Controls, left to right (2026-11-07) |
|---|---|
| Code | Visual Basic, Macros, then a stacked column: Record Macro, Use Relative References, Macro Security |
| Add-ins | Add-ins, Excel Add-ins, COM Add-ins |
| Controls | Insert (a menu), Design Mode, then a stacked column: Properties, View Code, Run Dialog |
| XML | Source, then a stacked column: Map Properties, Expansion Packs, Refresh Data, beside Import and Export |

Excel also sizes these as big buttons with the stacked entries as small buttons, wraps "Add-ins"
labels onto two lines, and paints the group names under each group. GridCraft's ribbon card paints
no group captions on any tab, so the tab keeps the same order and grouping but relies on the
separator lines the rest of the ribbon uses.

## Entries in GridCraft

`crates/ui-egui/src/ribbon/developer.rs` draws the tab; `Macros` is the only live control.

| Entry | State | Notes |
|---|---|---|
| **Macros** | **Real** | Runs `view.macros`; with no parameters the UI maps that to the Macros dialog (also Alt+F8), which lists this workbook's macros and scripts. `run` still returns the script list to programmatic callers |
| Visual Basic | Stub | Disabled; the editor is not implemented (#2960) |
| Record Macro | Stub | Disabled here; recording works from the Automate tab today (#2960 covers owning it here) |
| Use Relative References | Stub | Disabled; the Automate tab has the working toggle (#2960) |
| Macro Security | Stub | Disabled; no macro-trust settings exist (#2960) |
| Add-ins | Stub | Disabled; GridCraft extends through MCP and scripts, not add-ins (#2960) |
| Excel Add-ins | Stub | Disabled (#2960) |
| COM Add-ins | Stub | Disabled (#2960) |
| Insert | Stub | Disabled; form controls are drawn from **Insert › Controls** in this build (#2960) |
| Design Mode | Stub | Disabled; no design mode (#2960) |
| Properties | Stub | Disabled here; a selected form control's Format Control dialog edits it today (#2960) |
| View Code | Stub | Disabled (#2960) |
| Run Dialog | Stub | Disabled (#2960) |
| Source | Stub | Disabled; no XML maps (#2960) |
| Map Properties | Stub | Disabled (#2960) |
| Expansion Packs | Stub | Disabled (#2960) |
| Refresh Data | Stub | Disabled (#2960) |
| Import | Stub | Disabled (#2960) |
| Export | Stub | Disabled (#2960) |

Every stub is drawn with the widgets' disabled state (`ui.add_enabled_ui(false)`, dimmed) and
never calls a command, so clicking one does nothing. The stub tooltips name the issue.

## Reserved scripting languages

The issue asks for Python, Rust and VBA alongside Office Scripts. None is implemented: GridCraft
runs no Python, Rust or VBA, and VBA projects in an opened file are preserved on save but not run.
Office Scripts (TypeScript) are the implemented engine and run from the **Automate** tab (#3527).
Because Excel's Developer tab has no language control, this tab does not invent one; the reserved
engines are documented here and tracked in #2960.

## Catalog and keytips

`crates/engine/src/catalog.rs` lists the tab's Excel commands with `developer.*` ids (the ids a
future implementation will register) so `cargo xtask parity` reports them as missing until they
land. `Macros` is already catalogued as `view.macros` under View, and the implemented form controls
are catalogued as `insert.button`, `insert.spinButton`, `insert.slider` and `control.format` under
Developer › Controls. The tab's keytip is **Alt+L** (`L`), Excel's documented Developer tab access
key, added to `crates/ui-egui/src/keytips/keytips.json`; deeper Developer letters are left for when
the entries run commands.

## Tests

`crates/ui-egui/tests/it/developer_tab.rs` holds the tab to these promises: every entry renders,
the groups keep Excel's order and the card fits, all stubs are disabled and clicking one opens
nothing or starts no recording, the tooltips name #2960, and `Macros` opens the Macros dialog.
