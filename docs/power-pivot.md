# Power Pivot

The Data tab's **Power Pivot** button opens the data-model window (`powerPivot`). It follows
Excel's Power Pivot layout, but every model operation runs through the `dataModel.*` engine
commands, so the UI, agents and the control channel see the same model.

The model is **session state**: `dataModel.load` takes a JSON model as `path` (a file the native
host reads), `json` (the JSON text) or `model` (an already-parsed model object) — browser hosts
use `json` or `model`, since wasm has no filesystem. Measure and relationship edits update the
engine session only. They are not written to the workbook and are not undoable — the one
exception is `dataModel.createPivot`. When the workbook already has `xl/model/item.data`, it
writes the session model's measures that have a home table into that part before attaching the
PivotTable's cache. Measures without a home table are not written, and nothing is written when
the workbook has no such part.

## Window layout

- In-window tabs: **File**, **Home**, **Design**, **Advanced**. There is no Diagram tab.
- The canvas is always either **Data View** or **Diagram View** (`view` = `Data` or `Diagram`).
  Home, Design and Advanced are ribbon tabs over that same canvas; switching tabs never swaps
  the canvas. File replaces the canvas with a backstage pane.
- `tab` = `File` | `Home` | `Design` | `Advanced` and `view` = `Data` | `Diagram` are dialog
  values, so they can be driven from the control channel.
- The header shows whether a model is loaded and the table/measure counts; the File pane also
  lists the relationship count.

## Home ribbon

Excel's group order is preserved: Clipboard, Get External Data, Refresh, PivotTable, Formatting,
Sort and Filter, Find, Calculations, View. Unsupported commands are visible but disabled with an
explanatory tooltip.

| Group | Works | Disabled |
| --- | --- | --- |
| Clipboard | — | Paste, Paste Append, Paste Replace, Copy |
| Get External Data | **Existing Connections** (connection overlay) | From Database, From Data Service, From Other Sources |
| Refresh | **Refresh** (`dataModel.status` plus the selected table's rows) | — |
| PivotTable | — | PivotTable (Excel's button is disabled here; the worksheet command `dataModel.createPivot` creates the report instead) |
| Formatting | **Data Type** (selected column's type), **Format** for the selected measure (`dataModel.setFormat`), currency/percent/comma/increase/decrease decimals applied to the measure's format | — |
| Sort and Filter | **Sort A to Z**, **Sort Z to A**, **Clear Sort**, **Filter** by value, **Clear All Filters**, **Sort by Column** (through a relationship) — all session view state | — |
| Find | **Find** (`dataModel.find`): table/column scope, case, returns `{table, column, row, value}` matches | — |
| Calculations | **AutoSum** — adds a `SUM` measure for the selected numeric column (`dataModel.addMeasure`) | Create KPI |
| View | Data View, Diagram View, Calculation Area, Show Hidden (now changes the grid) | — |

- Data View / Diagram View and Calculation Area take effect immediately.
- **Show Hidden** is a real toggle: `dataModel.setShowHidden` makes the grid show hidden columns,
  and `dataModel.setColumnHidden` (Design › Columns › Hide) sets a column's hidden flag in the model.
- Sort and filter commands mutate the window's view state (`Session::dax_view`), never the model:
  they change which rows the Data View shows and in what order; clearing restores model order.
- The Formatting group edits the selected measure through `dataModel.setFormat`: a preset (General,
  Number, Currency, Percent, Comma) or a literal format, with optional decimals. There is no date
  preset; pass a literal date format code instead. Currency, percent, comma and the decimal buttons
  apply the corresponding preset.

## Design ribbon

Groups: Columns, Calculations, Relationships, Table Properties, Mark as Date Table, Date Table,
Edit. The Data View/Diagram canvas stays visible below this ribbon.

- **Columns — Add** opens Add Measure for the selected table; **Hide/Unhide** works
  (`dataModel.setColumnHidden`). Delete, Freeze and Width stay disabled: model columns cannot be
  deleted, frozen or resized.
- **Calculations — Insert Function** opens the Function Picker; **Calculation Options** opens the
  Automatic/Manual dialog. The Picker searches Excel-style categories and inserts a filled
  template into the formula bar (or the Add Measure formula field) using the first loaded table's
  name, a numeric column, a text column and the first measure. The catalogued functions live in
  `crates/dax/src/functions.rs`; which families a build dispatches is `linked_families()` in
  `crates/dax/src/eval.rs`, alongside the functions the evaluator core handles itself: the aggregation
  set (`SUM`, `COUNT`, …) before dispatch, and a scalar fallback (`DATE`, `POWER`, …) after family dispatch
  when no linked family claims the name.
- **Relationships — Create Relationship…** and **Manage Relationships** both work (see dialogs).
- **Table Properties** opens the read-only properties dialog.
- **Mark as Date Table** works (`dataModel.markDateTable`, `dataModel.unmarkDateTable`): it validates
  that the chosen column holds contiguous, unique dates before it stores the mark on the model.
  **Date Table**, **Undo** and **Redo** stay disabled; model edits commit immediately.

## Advanced ribbon

Groups: Perspectives, Show Implicit Measures, Summarize By, Reporting Properties.

- **Show Implicit Measures** is a real toggle (`dataModel.setShowImplicitMeasures`): with it on, the
  Calculation Area lists an implicit measure per numeric column (its default aggregation) and per
  non-numeric column (`COUNT(column)`, the column's non-blank values), matching Excel's default
  summarization. Hidden columns and columns summarized by `None` get no implicit measure. One gap:
  `COUNT` skips `Scalar::Boolean` values, so a Boolean column has no implicit count measure where
  Excel would show its non-blank count.
- **Summarize By** works (`dataModel.setSummarizeBy`): Sum, Average, Count, Min, Max or None on the
  selected column, which is what the implicit measure uses.
- Perspectives, Table Detail, Table Behavior and Image URL are disabled: the model engine has no
  perspectives or reporting-property metadata.

## Data View

- **Formula bar**: the name box selects one of the table's columns and shows a selected measure
  as `[Measure]`; it is followed by **X** (clear), **✓** (evaluate) and **fx** (Function Picker),
  then the DAX expression. Enter evaluates through `dataModel.evaluate`; a bare `[Name]` evaluates
  that measure. The last result is shown as `Scalar: …` or `Measure: …`.
- **Grid**: row-number gutter, column headers with filter menus, striped rows, both scroll
  directions, and a green **Add Column** header that opens Add Measure. A header filter menu lists
  up to 50 distinct values plus **All** and **Clear Filter**; it filters only the displayed rows.
  At most 200 rows are shown, with a count when more remain.
- **Calculation Area** (toggleable): a Name / Formula / Format grid of measures. **Add Measure**
  opens the measure dialog, **Update** edits the selected measure, and **Delete** calls
  `dataModel.deleteMeasure` immediately. The format column shows the status `format` (or
  `General`).
- **Footer**: table tabs for the loaded tables, plus the bottom-right **Data View** /
  **Diagram View** toggle.

## Diagram View

Table cards show the table name and up to 12 column names (an overflow row counts the rest), and
relationships draw orthogonal connector lines between cards. At most 60 tables are drawn. The
view offers **Fit**, **Refresh** and **Data View**; with no relationships it shows the cards alone.

## File tab

- **Open Model…** opens the model-file overlay.
- **Existing Connections** opens the same overlay in its connected-model presentation.
- **Switch to Advanced Mode** selects the Advanced tab.
- **Close** and **Exit** close the window.

## Dialogs

All overlays are reported through `overlay` = `""` | `openModel` | `existingConnections` |
`manageRelationships` | `createRelationship` | `tableProperties` | `calculationOptions` |
`addMeasure` | `find` | `sortByColumn`.

- **Add Measure** / **Update Measure** (`measureDialogMode` = `Add` | `Update`): name, home table,
  formula and format. Add calls `dataModel.addMeasure`; Update calls
  `dataModel.updateMeasure` with `new_name` only when the name changed, the formula when it is
  non-blank, and `format: null` when the format field is blank. Update is prefilled from the
  selected measure.
- **Manage Relationships**: lists From, To and cross-filter (Both/Single) for every relationship.
  **Create Relationship…** switches to that dialog; **Delete** calls
  `dataModel.deleteRelationship` for the selected endpoints.
- **Create Relationship**: From/To table and column pickers plus **Make this relationship
  bidirectional**; **Create** calls `dataModel.addRelationship`.
- **Table Properties**: read-only table name, row count and column/type table.
- **Calculation Options**: Automatic/Manual radio buttons. Manual is a window setting only —
  measure and relationship commands still commit immediately.
- **Open Model / Existing Connections**: **Browse…** picks the model JSON (the browser has no
  filesystem, so the picker's bytes go straight to `dataModel.load` as `json`; native hosts pass
  the chosen path), or paste the JSON and press **Load**. A typed filesystem path is treated as
  JSON text, so use **Browse…** for a file. Errors stay in the dialog.

## Engine command contract

| Command | Parameters | Result |
| --- | --- | --- |
| `dataModel.load` | `{path}` on native hosts, or `{json}` / `{model}` (browser picker, agents) | status |
| `dataModel.status` | `{}` | status |
| `dataModel.evaluate` | `{formula}` or `{measure}`, `table?: bool` | `{value, display}` — plus `format` when called with `measure` — or `{columns, rows}` |
| `dataModel.pivot` | `{measure, rows, columns?}` | `{columns, rows}` plus `display_rows` and `format` |
| `dataModel.addMeasure` | `{name, formula, table?, format?}` | status |
| `dataModel.updateMeasure` | `{name, formula?, format?, new_name?}` | status |
| `dataModel.deleteMeasure` | `{name}` | status |
| `dataModel.addRelationship` | `{from_table, from_column, to_table, to_column, bidirectional?, inactive?}` | status |
| `dataModel.deleteRelationship` | `{from_table, from_column, to_table, to_column}` | status |
| `dataModel.view` | `{table, show_hidden?}` | `{columns, rows, text_rows, row_numbers, hidden, sort, filters, truncated}` (view state; at most 5,000 rows, `truncated` says more exist) |
| `dataModel.find` | `{query, table?, column?, case?}` | `{matches: [{table, column, row, value}], truncated}` (at most 500 matches; `truncated` says more exist) |
| `dataModel.sortTable` | `{table, column, descending?}` | view state (session only) |
| `dataModel.clearSort` | `{table}` | view state (session only) |
| `dataModel.setFilter` | `{table, column, values: [text]}` (empty clears) | view state (session only) |
| `dataModel.clearFilters` | `{table}` | view state (session only) |
| `dataModel.sortByColumn` | `{table, by_table, by_column, column?, descending?}` | view state (session only) |
| `dataModel.setShowHidden` | `{show}` | status |
| `dataModel.setShowImplicitMeasures` | `{show}` | status |
| `dataModel.setFormat` | `{name, format?, preset?, decimals?}` (`decimals` is a delta; ignored when `format` or `preset` is given) | status (also applies to `evaluate`/`pivot` output) |
| `dataModel.setColumnHidden` | `{table, column, hidden?}` | status |
| `dataModel.setColumnType` | `{table, column, data_type}` | status (rejects a value the new type cannot hold) |
| `dataModel.setSummarizeBy` | `{table, column, summarize_by: Sum\|Average\|Count\|Min\|Max\|None}` | status |
| `dataModel.markDateTable` | `{table, column}` | status (the column must be contiguous, unique dates) |
| `dataModel.unmarkDateTable` | `{table}` | status |
| `dataModel.validate` | `{}` | `{tables, rows, relationships, findings}` — referential integrity, cardinality, blank keys, cycles |
| `dataModel.createPivot` | `{measure?, values?: [measure or {column, aggregate}], row_fields?, column_fields?, filters?, name?, sheet?, at?, layout?, row_subtotals?, column_subtotals?, grand_totals?, replace?}` | `{name, sheet, at, rows, columns, values, cached}`; a string value is a measure, `{column, aggregate: Sum\|Average\|Count\|Min\|Max}` is Excel's implicit measure for that column (built for the call only, not added to the model); `measure` may be omitted when `values` names a field; `replace` clears the previous report of that name and re-applies the kept PivotTable |
| `dataModel.pivotFields` | `{name?}` | `{pivots: [{name, sheet, at, range}], model: {tables, measures, relationships}, selected}` — what the field-list pane reads; `at` is the report body (below the page fields), `range` is the whole report including its page-field rows |
| `insert.modelPivotTable` | same as `dataModel.createPivot` | same (Insert › Tables entry point) |

`dataModel.createPivot`, `dataModel.pivotFields` and `insert.modelPivotTable` come with the PivotTable
Fields slice (`crates/engine/src/cmd/dax_pivot.rs`). They belong to this contract — the window's PivotTable
group and that slice's field pane drive the same reports — so they are listed here, but a build carrying the
window without that slice does not register them.

`status` is:

```json
{
  "loaded": true,
  "tables": [{"name": "Sales", "columns": [{"name": "Amount", "type": "Double"}], "rows": 4}],
  "measures": [{"name": "Total Sales", "table": "Sales", "formula": "SUM(Sales[Amount])", "format": "Currency"}],
  "relationships": [{"from_table": "Sales", "from_column": "Region", "to_table": "Regions",
                     "to_column": "Region", "bidirectional": false, "inactive": false}]
}
```

- With no model loaded, `status` returns `{"loaded": false, "tables": [], "measures": []}`. Every
  editing command is gated on a loaded model and appears disabled in the command catalog until
  `dataModel.load` (or a workbook adapter) fills the session model.
- The model JSON has `tables` (name, columns with `name`/`data_type`, optional inline `rows`),
  `measures` (name, formula, optional `table` and `format`) and `relationships` (the four
  endpoints plus optional `bidirectional` and `inactive`). `status` reports each column's
  `data_type` as `type`. `load` re-parses every measure formula and rejects duplicate
  table/column names and relationships naming missing tables or columns.
- Edits are atomic: a duplicate measure name or relationship, unknown table/column, unparsable
  formula or blank required field rejects the command, and each command revalidates the whole
  model before storing it, so a failure leaves the model untouched. Names match
  case-insensitively; stored status uses the model's spelling.
- `addMeasure` requires `name` and a non-blank `formula`. `updateMeasure` requires `name`; an
  absent `format` leaves it unchanged, while `format: null` clears it. `deleteMeasure` requires
  `name`. `deleteRelationship` identifies a relationship by its four endpoints.
- `setFormat` applies at most one of `format`, `preset` and `decimals`, in that precedence order:
  `decimals` is ignored when the same call supplies `format` or `preset`. `decimals` is a change in
  decimal places (−10 to 10), not an absolute count, applied to the measure's current format
  (`0.00` when it has none); apply a preset and then adjust `decimals` in a separate call.
- `evaluate` accepts a formula or a measure name; `table: true` asks for a table result. A bare
  table name evaluates to its rows. The measure's format reaches `display` only on the `measure`
  path: `{measure}` returns `{value, display, format}` with `display` formatted, while `{formula}`
  returns `{value, display}` and `display` is the plain value text (no `format` field, no measure
  format applied). `pivot` groups one measure by row fields and optional column fields; it returns
  a table, not a worksheet PivotTable, and it carries both grids: `rows` stays unformatted and
  exact for agents, while the parallel `display_rows` holds formatted text strings and `format`
  reports the format code used.

## Parity and limits

| Excel Power Pivot | GridCraft |
| --- | --- |
| File/Home/Design/Advanced tabs, ribbon groups | Matches: same tab strip and group names; unsupported commands are visibly disabled with tooltips |
| Data View and Diagram View over one canvas | Matches; the view is a `view` value, not a tab |
| Data View formula bar, name box, row gutter, header filters, table tabs | Matches; header filters affect only the displayed rows |
| Calculation Area with measure Name/Formula/Format | Matches; measure `format` comes from `dataModel.status` |
| Add/Update/Delete Measure | Matches, wired to `dataModel.addMeasure` / `updateMeasure` / `deleteMeasure` |
| Create/Manage/Delete Relationship | Matches, wired to `dataModel.addRelationship` / `deleteRelationship` |
| Table Properties showing column types | Matches (read-only), from the status `type` field |
| Diagram cards and relationship lines | Matches, with a 60-table/12-column display cap |
| Get External Data / database and service imports | Missing: `dataModel.load` takes a JSON model only (no database or service importers) |
| Paste, copy, Import, existing worksheet connections | Missing in this window; worksheet commands are separate |
| Add/delete/rename model columns, data-type editing, freeze, width | Partial: **Hide/Unhide** and **Data Type** work (`setColumnHidden`, `setColumnType`); delete, rename, freeze and width are missing |
| Sort, filter and format model rows | Partial: sort/filter/find work on the window's view state (`sortTable`, `setFilter`, `sortByColumn`, `find`); they do not reorder model storage |
| Mark as Date Table / Date Table metadata | Partial: **Mark as Date Table** / **Unmark** validate and store the mark; the automatic Date Table command is missing |
| Perspectives, KPIs, hierarchies, Summarize By, reporting properties | Partial: **Summarize By** works (`setSummarizeBy`) and feeds implicit measures; perspectives, KPIs, hierarchies and reporting properties are missing |
| Undo/Redo, Find, AutoSum | Find works (`dataModel.find`) and AutoSum adds a `SUM` measure for the selected numeric column (`dataModel.addMeasure`); undo/redo are missing, so edits commit immediately |
| Calculation Options Manual mode | Window state only; commands still commit immediately |
| Show Hidden / Show Implicit Measures | Both work: hidden columns are model metadata, implicit measures are generated from each column's default summarization |
| Worksheet PivotTable field list and cache | Partial: `dataModel.createPivot` writes the report cells plus `pivotTable*.xml` / `pivotCacheDefinition*.xml` / `xl/connections.xml`, keeps them on save, and the parts survive save/reopen. Excel does not yet accept the generated PivotTable cache definition, so the file opens but the PivotTable itself does not refresh. The **PivotTable Fields pane** covers model pivots (`dataModel.pivotFields`): a searchable field tree with drag-and-drop between Rows/Columns/Filters/Values, tick boxes, a Summarize by menu on column values (Sum/Average/Count/Min/Max as an implicit measure), slicer cards that drive page filters, layout and grand totals, and Apply updates the pivot in place. A real on-sheet slicer object and a timeline are not there: the slicer model resolves columns against worksheet tables only, and `slicer`/`slicercache`/`timeline` parts are not preserved — see the limits below |
| Pivot Preview | Not present: the window has no separate preview surface |
| Model persistence and refresh from a source | Partial: `xl/model/item.data` is kept through read and save, and `dataModel.createPivot` writes the session model's measures into it before attaching the cache. Excel reopened a written package and listed and evaluated the written measures (through `CUBEVALUE`); modified VertiPaq column *values* are not applied by Excel, so a refreshed PivotTable is not supported. The model's *tables* still come from the workbook's own model, not from the session JSON, and a model created from JSON alone has no database part to write |

DAX support is partial: the catalog parses 484 functions. The evaluator core handles the aggregation
functions before dispatch, then the families linked in `crates/dax/src/eval.rs`'s `FAMILIES`, and finally a
scalar fallback of its own (`DATE`, `POWER`, …) for names no linked family claims; a call into a family that
is not linked therefore ends in the `not implemented` fallback. With every family linked, all 484 evaluate.
