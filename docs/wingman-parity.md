# Wingman capability map

Use the [command and control index](wingman-commands.md) for individual GridCraft menu entries. This table compares the operations in [Zed’s agent tools](https://zed.dev/docs/ai/tools) with what the desktop workbook agent can actually do. A workbook is the workspace; the filesystem is not.

| Zed tool or behavior | GridCraft equivalent | Boundary |
|---|---|---|
| `diagnostics` | `check_formula` before a write; `formula_diagnostics`, `inspect_formula` and `profile_range` after it | Entry checks, static references and cached values answer different questions. No guessed recalculation. |
| `find_path` | `list_objects` for named objects; `find_commands` for a caption, group, or command ID | Searches workbook objects or installed UI metadata, not paths on disk. |
| `grep` | `grep` against typed values, displayed text, formula source, comments or actual errors | Literal by default; optional regex, scope, sheet, range and continuation cursor. |
| `list_directory` | `workbook_info`, `list_objects`, `list_scripts` | Visible workbook scope; metadata is paged. |
| `read_file` | `read_range`, `inspect_cell`, `read_script` and explicit context mentions | No general file reader. Oversized previews do not truncate an edit's source. |
| `fetch` | A configured MCP read tool, under **External** | Separate connection and approval; no implicit browser. |
| `search_web` | A connected search provider, e.g. Localflame | Not a model-provider subscription or an automatic transfer of sheet data. |
| `copy_path` | `transfer_range` in copy or transpose mode | Native relative reference handling; collision and object checks. |
| `create_directory` | `add_sheet` to make a workbook container | No OS directory creation. |
| `delete_path` | `clear_contents`, `delete_rows`, `delete_columns` | Destructive worksheet actions require approval. No file deletion. |
| `edit_file` | `set_cell`, typed range/object controls, and native macro draft edits | Arguments shown before mutation; the workbook owns Undo. |
| `move_path` | `transfer_range` in move mode; `rename_sheet` or `rename_table` for names | Cut has different formula-reference semantics from copy then clear. |
| `write_file` | `set_values`, `save_script` for native scripts | Existing content, overwrite and protection rules are tool-specific. No file save on behalf of the user. |
| `terminal` | No built-in equivalent | Wingman cannot shell out, run a macro, or evaluate VBA. |
| `skill` | Tool schemas and the native menu/function catalogs provide local instructions | No skill-file loader. |
| `spawn_agent` | Switch between configured agents in the dock | A switch starts another conversation, not an autonomous child. |
| Agent panel and model selection | Resizable Wingman dock, explicit model/agent, thinking level where supported | The launcher is opt-in. No provider contact just from opening the pane. |
| Context and slash commands | Composer suggestions, `@selection`, sheet/range and macro context, local slash commands | Only the contexts and commands shown in completion are supported. |
| [Profiles and permissions](https://zed.dev/docs/ai/tool-permissions) | Chat only, Read only, Read / write, Read / write / MCP | Availability is distinct from approval. No persistent always-approve rule. |
| Context-specific tool palette | `change_tab` swaps schemas for the next request; optionally narrow to a named group | The agent’s ribbon is independent of the user’s. Discovery alone grants no permission. |
| Review and revert | Inspect exact arguments, approve a call or atomic `apply_edits` plan, undo/redo in the workbook | External actions are separate; they cannot be rolled back with Ctrl+Z. |
| Provider API | Configured Chat Completions and native Ollama, model discovery and manual reasoning effort | Discovery does not establish which optional protocols a server implements. |
| [MCP](https://zed.dev/docs/ai/mcp) | Explicitly connected stdio or HTTP servers, selected catalog, arguments and per-call confirmation | No automatic approval for a tool that describes itself as read-only. |
| [External agents / ACP](https://zed.dev/docs/ai/external-agents) | No ACP process host | An OpenAI-compatible model endpoint is not an ACP agent. |

## Working through the grid

| User operation | Agent path | What to check |
|---|---|---|
| Find every cell containing XYZ | Workbook → `grep` once; continue only if there is a cursor | Actual sheet coordinates; report skipped oversize fields. |
| Format a column as currency | Home → `number_format`, e.g. `format: "Currency"` | `inspect_format` distinguishes cell formatting from its typed numeric value. |
| Add two columns row by row | Formulas → `check_formula`; Home → enter one seed with `set_cell`, then `drag_fill` | Inspect the first and last formula; a fill shifts relative, not absolute, references. |
| Find bad inputs for a formula | Data → `profile_range`; Formulas → `inspect_formula` and `formula_diagnostics` | Numeric-looking strings, blanks, and cached errors are distinct. Type counts have row pagination. |
| Center a heading, wrap a paragraph, change font, color and borders | Home → `format_range`, `set_fill`, `set_ink`, `set_borders` | Explicit style properties, border placement and line style; `inspect_format` reads the result. |
| Unmerge text and center it across the former merge | Home → `unmerge_cells`, then `format_range` with `align.h: "CenterAcross"` | The text stays in its anchor. Merge refuses to discard populated non-anchor cells. |
| Move a block; rotate a column into a row | Home → `transfer_range` with `move` or `transpose` | Specify sheet, destination and any intended overwrite. Transpose is a copy, not a cut. |
| Turn a delimited column into a row | Data → `reshape_delimited` with `layout: "row"` | Quoted delimiters are fields, not separators. Parsed text is written literally, not as a formula. |
| Choose a chart for data | Insert → `recommend_charts`, then `create_chart` with that choice or `auto` | Inspect source, type and series using `inspect_chart`. Recommendations are native heuristics. |
| Pull AB comments into AC under a centered header | Review → `comments_to_column` | Native extraction uses complete threads; copy preserves the originals, move removes them. |
| Select, sort, then filter | `select_range`, Data → `sort_rows`, `enable_filter`, `filter_column` | Explicit columns and header; filters operate on their actual rectangular region. |
| Remove duplicate records or group the detail rows | Data → `remove_duplicates`, `group_outline`, `ungroup_outline` | Deduplication compacts standalone values, not tables or formula records. An outline changes grouping, not cell values. |
| Ask “what input would get me to this total?” | Data → `preview_goal_seek` | Native solver on a temporary copy. A result is a proposal, not a change to the input cell. |
| Add a dropdown or highlight duplicates | Data → `set_validation`; Home → `conditional_format` | Use `inspect_rules` for readback; clearing a rule is a separate operation. |
| Style an existing table or change its totals | Table Design → `inspect_table`, `set_table_style`, `table_banded_rows`, `table_total_function` | Address the table by name; workbook ownership and protected targets are checked. |
| Create or restyle a PivotTable | Insert → `create_pivot`; PivotTable Design → `inspect_pivots`, `set_pivot_layout` | Local source headers; create on a new sheet so existing cells are not overwritten. |
| Annotate a diagram | Draw → `add_ink_stroke` or Insert → `create_shape`; Format → `inspect_shapes` and object styling | Actual drawing object IDs and anchors, not simulated mouse events. |
| Set printed orientation or freeze headings | Page Layout → `set_page_layout`; View → `freeze_panes` | `inspect_layout` reads saved settings. Printing and print dialogs stay manual. |
| Edit a repeating native macro | Automate → read draft, propose a step edit, then user saves | No auto-run. Excel VBA can be drafted as text; its binary project is preserved, not executed. |

To tell whether a particular menu entry is executable, use its **Access** and **Wingman tool** columns in the [native inventory](wingman-commands.md), not just the presence of a command ID. An equivalent tool can cover part of an interactive dialog without exposing the entire dialog.

## What a formula check establishes

| Question | Evidence Wingman can read | Limitation to report |
|---|---|---|
| Will this input be stored as a formula? | Native parser, entry/arity rules, auto-closed parentheses and the intended cell’s Text number format | Supply the target cell; otherwise the checker cannot inspect its format. |
| Is a name, table or function unknown? | Bounded lookup against visible workbook catalogs and the native function registry | Local LET/LAMBDA bindings or an incomplete catalog scan are not proof of an undefined name. |
| Will a direct arithmetic operand fail? | The evaluator’s own number coercion on a visible cached scalar; zero denominator and existing error values are reported by cell | Potential warning, not evaluation: the branch might not run. Indirect, ranged and dynamic inputs are not inferred. |
| Is the proposed reference broken or self-referential? | Explicit broken-reference literals and direct references covering the target; inspect the dependency index for an existing cell | Direct references do not detect every cycle. Dynamic or missing index entries are reported separately. |
| What actually errored in the sheet? | Cached typed errors with `formula_diagnostics`, as distinct from ordinary strings containing an error label | A stale cache is not a new calculation. |
| Is this a data-entry or display problem? | `profile_range` counts types and gives example addresses; `inspect_format` reads number formats and mixed styles | Counts apply to the current page; a cell displayed as money or a date may store a number. |
