# Power Pivot

The Data tab's **Power Pivot** button opens the data-model window (`powerPivot`). It follows
Excel's Power Pivot layout, but every model operation runs through the `dataModel.*` engine
commands, so the UI, agents and the control channel see the same model.

The model is **session state**: `dataModel.load` reads a JSON model file, and measure and
relationship edits update the engine session only. They are not written to the workbook and are
not undoable.

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
| PivotTable | **PivotTable** — creates a worksheet PivotTable over the model (`dataModel.createPivot`) in the same window's workbook | — |
| Formatting | **Data Type** (selected column's type), **Format** for the selected measure (`dataModel.setFormat`), currency/percent/comma/increase/decrease decimals applied to the measure's format | — |
| Sort and Filter | **Sort A to Z**, **Sort Z to A**, **Clear Sort**, **Filter** by value, **Clear All Filters**, **Sort by Column** (through a relationship) — all session view state | — |
| Find | **Find** (`dataModel.find`): table/column scope, case, returns `{table, column, row, value}` matches | — |
| Calculations | — | AutoSum, Create KPI |
| View | Data View, Diagram View, Calculation Area, Show Hidden (now changes the grid) | — |

- Data View / Diagram View and Calculation Area take effect immediately.
- **Show Hidden** is a real toggle: `dataModel.setShowHidden` makes the grid show hidden columns,
  and `dataModel.setColumnHidden` (Design › Columns › Hide) sets a column's hidden flag in the model.
- Sort and filter commands mutate the window's view state (`Session::dax_view`), never the model:
  they change which rows the Data View shows and in what order; clearing restores model order.
- The Formatting group edits the selected measure through `dataModel.setFormat`: a preset (General,
  Number, Currency, Percent, Date) or a literal format, with optional decimals. Currency, percent,
  comma and the decimal buttons apply the corresponding preset.

## Design ribbon

Groups: Columns, Calculations, Relationships, Table Properties, Mark as Date Table, Date Table,
Edit. The Data View/Diagram canvas stays visible below this ribbon.

- **Columns — Add** opens Add Measure for the selected table. Delete, Freeze and Width are
  disabled: model columns cannot be deleted, frozen or resized.
- **Calculations — Insert Function** opens the Function Picker; **Calculation Options** opens the
  Automatic/Manual dialog. The Picker searches Excel-style categories and inserts a filled
  template into the formula bar (or the Add Measure formula field) using the first loaded table's
  name, a numeric column, a text column and the first measure. Catalogued and evaluated functions
  are listed in [DAX functions](dax-functions.md).
- **Relationships — Create Relationship…** and **Manage Relationships** both work (see dialogs).
- **Table Properties** opens the read-only properties dialog.
- **Mark as Date Table** works (`dataModel.markDateTable`, `dataModel.unmarkDateTable`): it validates
  that the chosen column holds contiguous, unique dates before it stores the mark on the model.
  **Date Table**, **Undo** and **Redo** stay disabled; model edits commit immediately.
- **Columns — Hide/Unhide** works (`dataModel.setColumnHidden`). Delete, Freeze and Width remain
  disabled: model columns cannot be deleted, frozen or resized.

## Advanced ribbon

Groups: Perspectives, Show Implicit Measures, Summarize By, Reporting Properties.

- **Show Implicit Measures** is a real toggle (`dataModel.setShowImplicitMeasures`): with it on, the
  Calculation Area lists an implicit measure per numeric column (its default aggregation) and per
  non-numeric column (`COUNTROWS`), matching Excel's default summarization.
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
`addMeasure`.

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
  the chosen path), or type a path / paste the JSON and press **Load**. Errors stay in the dialog.

## Engine command contract

| Command | Parameters | Result |
| --- | --- | --- |
| `dataModel.load` | `{path}` on native hosts, or `{json}` / `{model}` (browser picker, agents) | status |
| `dataModel.status` | `{}` | status |
| `dataModel.evaluate` | `{formula}` or `{measure}`, `table?: bool` | `{value}` or `{columns, rows}` |
| `dataModel.pivot` | `{measure, rows, columns?}` | `{columns, rows}` |
| `dataModel.addMeasure` | `{name, formula, table?, format?}` | status |
| `dataModel.updateMeasure` | `{name, formula?, format?, new_name?}` | status |
| `dataModel.deleteMeasure` | `{name}` | status |
| `dataModel.addRelationship` | `{from_table, from_column, to_table, to_column, bidirectional?, inactive?}` | status |
| `dataModel.deleteRelationship` | `{from_table, from_column, to_table, to_column}` | status |
| `dataModel.view` | `{table, show_hidden?}` | `{columns, rows, text_rows, row_numbers, hidden, sort, filters}` (view state) |
| `dataModel.find` | `{query, table?, column?, case?}` | `{matches: [{table, column, row, value}]}` |
| `dataModel.sortTable` | `{table, column, descending?}` | view state (session only) |
| `dataModel.clearSort` | `{table}` | view state (session only) |
| `dataModel.setFilter` | `{table, column, values: [text]}` (empty clears) | view state (session only) |
| `dataModel.clearFilters` | `{table}` | view state (session only) |
| `dataModel.sortByColumn` | `{table, by_table, by_column, column?, descending?}` | view state (session only) |
| `dataModel.setShowHidden` | `{show}` | status |
| `dataModel.setShowImplicitMeasures` | `{show}` | status |
| `dataModel.setFormat` | `{name, format?, preset?, decimals?}` | status (also applies to `evaluate`/`pivot` output) |
| `dataModel.setColumnHidden` | `{table, column, hidden?}` | status |
| `dataModel.setColumnType` | `{table, column, data_type}` | status (rejects a value the new type cannot hold) |
| `dataModel.setSummarizeBy` | `{table, column, summarize_by: Sum\|Average\|Count\|Min\|Max\|None}` | status |
| `dataModel.markDateTable` | `{table, column}` | status (the column must be contiguous, unique dates) |
| `dataModel.unmarkDateTable` | `{table}` | status |
| `dataModel.validate` | `{}` | `{tables, rows, relationships, findings}` — referential integrity, cardinality, blank keys, cycles |
| `dataModel.createPivot` | `{measure, values?, row_fields?, column_fields?, filters?, name?, sheet?, at?, layout?, row_subtotals?, column_subtotals?, grand_totals?, replace?}` | `{name, sheet, at, rows, columns, values, cached}`; `replace` clears the previous report of that name and re-applies the kept PivotTable |
| `dataModel.pivotFields` | `{name?}` | `{pivots: [{name, sheet, at}], model: {tables, measures, relationships}, selected}` — what the field-list pane reads |
| `insert.modelPivotTable` | same as `dataModel.createPivot` | same (Insert › Tables entry point) |

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
- `evaluate` accepts a formula or a measure name; `table: true` asks for a table result. A bare
  table name evaluates to its rows. `pivot` groups one measure by row fields and optional column
  fields; it returns a table, not a worksheet PivotTable.

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
| Get External Data / database and service imports | Missing: `dataModel.load` reads a JSON model file only |
| Paste, copy, Import, existing worksheet connections | Missing in this window; worksheet commands are separate |
| Add/delete/rename model columns, data-type editing, freeze, width | Partial: **Hide/Unhide** and **Data Type** work (`setColumnHidden`, `setColumnType`); delete, rename, freeze and width are missing |
| Sort, filter and format model rows | Partial: sort/filter/find work on the window's view state (`sortTable`, `setFilter`, `sortByColumn`, `find`); they do not reorder model storage |
| Mark as Date Table / Date Table metadata | Partial: **Mark as Date Table** / **Unmark** validate and store the mark; the automatic Date Table command is missing |
| Perspectives, KPIs, hierarchies, Summarize By, reporting properties | Partial: **Summarize By** works (`setSummarizeBy`) and feeds implicit measures; perspectives, KPIs, hierarchies and reporting properties are missing |
| Undo/Redo, Find, AutoSum | Find works (`dataModel.find`); undo/redo and AutoSum are missing; edits commit immediately |
| Calculation Options Manual mode | Window state only; commands still commit immediately |
| Show Hidden / Show Implicit Measures | Both work: hidden columns are model metadata, implicit measures are generated from each column's default summarization |
| Worksheet PivotTable field list and cache | Partial: `dataModel.createPivot` writes the report cells plus `pivotTable*.xml` / `pivotCacheDefinition*.xml` / `xl/connections.xml`, keeps them on save, and the result **opens, refreshes and survives save/reopen in Excel**. The **PivotTable Fields pane** now exists for model pivots (`dataModel.pivotFields`): tables/columns/measures with tick boxes, Rows/Columns/Filters/Values areas, layout and grand totals, and Apply updates the pivot in place. It is not yet Excel's drag-and-drop tree with search |
| Pivot Preview | Not present: the window has no separate preview surface |
| Model persistence and refresh from a source | Partial: `xl/model/item.data` is kept through read and save, and `dataModel.createPivot` writes the session model's measures into it before attaching the cache, so Excel evaluates them on refresh. The model's *tables* still come from the workbook's own model, not from the session JSON, and a model created from JSON alone has no database part to write |

DAX support is partial: the catalog parses 484 functions and every one of them is evaluated. See
[DAX functions](dax-functions.md) for the per-category breakdown.
