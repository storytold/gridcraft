# Power Pivot (prototype)

The Data tab's **Power Pivot** button opens an Excel-shaped data-model window.
Everything goes through the `dataModel.*` engine commands, so agents and the
control channel see the same model the dialog edits.

## Layout

- **Home** — Data View: a DAX formula bar, the selected table's headed data grid,
  a Calculation Area grid for measures (Name / Formula / Format), and table tabs
  along the bottom. The pivot preview is a secondary floating surface opened by
  **Pivot Preview**, not the main view.
- **Design** — measure list and draft editor, field list, **Manage Relationships**
  (read-only relationships from `dataModel.status`), and **Functions**.
- **Diagram** — table cards (name + columns) and orthogonal relationship
  connector lines. **Fit** scales the cards to the window.
- Ribbon groups mirror Excel: Home = View / Calculations / Data; Design = Tables /
  Calculations; Diagram = View.

## Function Library

**Functions** opens a searchable picker with Excel's categories. Choosing a
function writes a parseable template into the active formula field, using the
first loaded table/column and measure where possible (`Sales[Amount]`,
`Sales[Region]`, `[Total Sales]`). The category → function → template table is
`LIBRARY_FUNCTIONS` in `crates/ui-egui/src/dialogs.rs`.

## Limits / visual approximations

- Add Measure and Delete are disabled placeholders: the engine has no command to
  persist measure edits. The Design formula is a draft that **Evaluate** runs.
- Measure `Format` is blank because `dataModel.status` does not expose it yet.
- A model path field is kept in the dialog (Excel opens models through the file
  workflow), so the prototype can load the JSON model directly.
- Relationships are read-only; there is no relationship editor.
- The Function Library templates are canonical DAX and insert correctly; only a
  representative subset is exercised at runtime by the interaction test.
