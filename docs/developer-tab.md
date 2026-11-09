# Developer tab

The Developer tab is a **UI skeleton** for Excel's Developer tab (#2960). It shows Excel's groups
and controls in their observed order, wires the one entry GridCraft already implements (`Macros`),
and renders every other entry as a disabled stub whose tooltip says **"In development — tracked in
#2960"**. It ships no scripting engine and runs no Python, Rust or VBA, and it has no Office
Scripts compatibility: the **Automate** tab runs GridCraft's own recorded macros (#3527).

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
| **Macros** | **Real** | Runs `view.macros` with `{"dialog": true}`, which opens the Macros dialog (also Alt+F8) and still answers with the workbook's script list. `{}` and `null` are the query alone: they return the list and open nothing, so a programmatic caller never has to guess |
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
runs no Python, Rust or VBA. A readable VBA project in an OOXML file is kept on open and carried
unchanged when the workbook is saved as `.xlsm` or `.xltm`; saving as `.xlsx` or `.xltx` omits it.
Office Scripts (TypeScript) are not compatible either: the **Automate** tab runs GridCraft's own
macros, which replay recorded command arrays, not TypeScript (#3527). Because Excel's Developer tab
has no language control, this tab does not invent one; the reserved engines are documented here and
tracked in #2960.

## Commands: `view.macros`, the reserved ids and the one id convention

`view.macros` is the tab's only live command and its schema is the same in the ribbon and in a
programmatic call (`dialog` is a UI request: with no UI attached the query answers and no dialog
opens, as with every other dialog-opening command):

```text
{"dialog": true}                          → the list, and the Macros dialog opens
{} / null / no params / other params      → the list, and nothing opens
the list: [{"name", "steps", "trusted"} | {"name": "(VBA)", "kind": "vba", "size", "executed": false}]
```

`dialog: true` is the explicit request the Macros button makes; it opens the Macros dialog as well
as answering the query. Any other params are ignored, so `{}`, `null` and unrelated params are
unambiguously the query.

One id convention holds for the whole tab (`crates/engine/src/catalog.rs`, enforced by
`developer_rows_use_one_namespace_convention`):

- A control whose command already exists keeps that command's id and does not get a second
  `developer.*` name: `view.macros` (the Macros dialog, also Alt+F8) and the form controls
  `insert.button`, `insert.spinButton`, `insert.slider` and `control.format` (Properties).
  `catalog::DEVELOPER_SHARED` records where each is shared from.
- An entry with no implementation yet reserves a `developer.*` id. `catalog::RESERVED` lists the
  17; they are registered as commands with `status: "not_implemented"` (not a second set of
  catalog rows), so a programmatic call answers a defined error instead of "unknown command":

```console
$ gridcraft-cli run --cmd 'developer.visualBasic={}'
gridcraft-cli: step 1 (--cmd, developer.visualBasic): NOT_IMPLEMENTED: `developer.visualBasic` is reserved for a feature that is not built yet (#2960)
```

`engine.commands` reports each of them with `status: "not_implemented"` and `enabled: false` (with
that reason in `disabled_reason`), they appear in no menu, and `cargo xtask parity` counts them as
missing — reserving an id never inflates the checklist.

The tab's keytip is **Alt+L** (`L`), Excel's documented Developer tab access key, added to
`crates/ui-egui/src/keytips/keytips.json`; deeper Developer letters are left for when the entries
run commands.

## Tests

`crates/ui-egui/tests/it/developer_tab.rs` holds the tab to these promises: every entry renders,
the groups keep Excel's order and the card fits, all stubs are disabled and clicking one opens
nothing or starts no recording, keyboard activation reaches none of them (a disabled control takes
no focus; `developer_stubs_stay_inert_under_keyboard_activation`), the tooltips name #2960,
`Macros` opens the Macros dialog by mouse or Enter, and **Alt+L** switches to the tab
(`alt_l_switches_to_the_developer_tab`). `crates/ui-egui/tests/it/ribbon_layout.rs`
(`a_narrow_window_scrolls_the_tab_row_to_its_last_tab`) scrolls the tab row in a 600 px window and
clicks the Developer tab at its end. `crates/ui-egui/tests/it/macros_ui.rs`
(`view_macros_answers_the_script_list_and_opens_the_dialog_only_when_asked`) covers the parameter
forms above, and the engine's `view_macros_answers_the_script_list_for_every_params_form` covers
them with no UI at all.
