# Plug-ins

GridCraft runs WebAssembly plug-ins through the shared `craft-plugin` runtime of the Craft apps
([RFC-0046](https://github.com/storytold/craft-contrib/blob/main/rfcs/0046-shared-plugin-runtime.md)).
A plug-in is a `.craftplugin` bundle: a manifest (`craft-plugin.json`) and a sandboxed WebAssembly
module. It works on the open workbook through the same commands as the ribbon, the CLI, the
control channel and MCP, and nothing else. The ABI, the manifest and the bundle format are in the
[craft-plugin README](https://github.com/Dasemu/craft-libs/blob/93428fbe5d11b735365fa87068a9e1fc85fb3550/crates/plugin/README.md)
(the revision GridCraft pins until the crate lands in craft-libs).

A manifest names GridCraft in `apps` (`"apps": {"gridcraft": {"api": 6}}`); GridCraft runs the
`command` kind only and refuses manifests that name no app.

## Using plug-ins

**Automate › Plug-ins** opens the Plug-ins dialog; the button's menu lists every installed
plug-in's commands. A command with parameters asks for them first, in a form built from its
manifest. The dialog lists the installed plug-ins (run a command, remove one), installs from a
`.craftplugin` file, a GitHub repository (`owner/repo` or `owner/repo@tag`) or a catalog, and
checks for updates. Downloads run on a worker thread.

Before anything is installed, the confirmation shows the plug-in's name, version, author, kind,
access (read, or read and edit), the commands it asked for, where it comes from and the bundle's
SHA-256, and says plainly when a repository is in no catalog you use. The bundle reviewed is the
one installed: it is held, not downloaded again, and the install names its SHA-256.

The desktop app keeps plug-ins in `Plug-ins/` in the settings folder, beside `prefs.json`; their
settings and the catalogs added are kept in `prefs.json`. The CLI, MCP and runs without
preferences (`GRIDCRAFT_NO_PREFS`) keep plug-ins for the session only and install from files,
base64 data and `file://` catalogs; the desktop app also downloads over HTTPS (catalogs and
releases on GitHub's hosts only, pinned by size and SHA-256).

## Commands

| Command | Params |
|---|---|
| `plugin.list` | `{}` → installed plug-ins, problems, catalogs, `stored`, the last run |
| `plugin.install` | `{path}` · `{data: base64, name?}` · `{repo: "owner/repo[@tag]"}` · `{catalog, id, version?}`, each with `review?` (describe it, install nothing) and `sha256?` (install only that bundle) |
| `plugin.update` | `{id?, apply?}` — updates that ask for other permissions wait for a confirmation |
| `plugin.remove` | `{id}` — removes it and its settings |
| `plugin.run` | `{id, command?, params?, fuel?}` — one undo step on the active workbook |
| `plugin.catalog.list` | `{url?}` — the packages GridCraft can run |
| `plugin.catalog.add` | `{url, remove?}` — `https://` on GitHub's hosts, or `file://` |

Macros and Automate scripts can't run `plugin.*` steps, and `plugin.run` isn't recorded.

## What a run may do

- **One undo step.** A run's commands become one undo step named after the plug-in. A run that
  fails (a trap, an exhausted instruction budget, a refusal it passes on, a non-zero status) leaves
  the workbook, the selection, the undo history and the plug-in's settings as they were. Only
  `plugin.run` is journaled, not the commands it ran.
- **Read access is checked after every call.** Each call runs in its own undo group inside the
  run's. One that edits the workbook from a plug-in that asked for `read` access, opens a dialog or
  the cell editor, or switches to another workbook is rolled back and refused.
- **An allowlist.** A plug-in may call only the commands in `ALLOWED` (`crates/engine/src/plugin.rs`):
  reading cells, the selection and the active sheet, editing cells and ranges, formatting, rows,
  columns and sheets, names, sorting and filtering, outlines, validation, what-if tools, tables,
  charts, PivotTables, slicers, objects, comments and notes, page layout and the sheet's view
  settings. A command added to GridCraft later is refused until it is reviewed. A manifest's
  `permissions.commands` narrows the list further.
- **Refused, always:** opening, saving, importing, exporting and printing files (`file.*`, Insert ›
  Picture, the sheet background, the Solver's model files); external data and its connections
  (Get Data, From Text/CSV, From Table/Range, Refresh All, Queries & Connections); macros,
  Automate scripts, Python cells and form controls; other workbooks and windows; undo and redo;
  the clipboard and Format Painter; preferences, the app and plug-ins themselves (`plugin.*`);
  links that leave the app; sheet and workbook protection; the Solver and Data Analysis.
- **No files.** A call whose parameters name a file (`path`, `file`, `folder`…) is refused, and
  every call runs inside the scripts' file sandbox, so no command reads or writes a file for a
  plug-in. The control channel's token, kept in a file and the app's environment, is out of reach.
- **Bounded.** A run may edit the workbook at most 50 times (the undo history keeps 100 steps);
  a call may write values into at most 100,000 cells and a run into 500,000, so no plug-in fills
  a whole column. Settings are at most 64 KiB per plug-in, keys at most 128 characters. The
  sandbox bounds instructions, memory, calls and messages (craft-plugin's `Limits`).

## Not yet

Plug-in panels (`ui_show`) aren't shown: `plugin.run` says so in its `panel` field. `saveFile` is
not offered. The web build installs from files only and keeps plug-ins for the session.
