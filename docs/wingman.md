# Wingman

Wingman is off by default. Enable **File → Options → Features → Show Wingman pane**, then choose **Wingman** at the far right of the desktop ribbon or in command search. The tile stays in place when the ribbon groups scroll and becomes a compact button when the ribbon collapses. Drag the panel's left edge to resize it. Each workbook window has its own threads and permissions. Opening the panel does not contact a model.

To remove Wingman from the ribbon, uncheck **File → Options → Features → Show Wingman pane**. This closes the dock and cancels its model requests and MCP connections in every open workbook window. Turning it back on restores the launcher without opening a pane.

## Connect a model

Choose **Open JSON**. GridCraft creates `wingman.json` beside its own `ui.json` in the active desktop profile, leaving any existing file untouched. Enter the URL and model ID your server uses, save the file and choose **Reload**. With a discovery-only configuration, choose **Find models** to ask the configured servers for their model lists. For instance, this configuration works without model discovery:

```json
{
  "language_models": {
    "openai_compatible": {
      "desk": {
        "api_url": "http://127.0.0.1:8000/v1",
        "auto_discover": false,
        "available_models": [
          { "name": "your-served-model-id", "display_name": "Local model", "max_tokens": 32768, "capabilities": { "tools": true } }
        ]
      }
    }
  },
  "agent": { "default_model": { "provider": "desk", "model": "your-served-model-id" }, "default_profile": "ask" },
  "wingman": {
    "agents": {
      "Sheet editor": {
        "provider": "desk", "model": "your-served-model-id",
        "default_profile": "write", "instructions": "Explain formula changes in plain language."
      }
    }
  }
}
```

Use your server's model ID in all three places. Multiple servers, models and named agents can coexist. Choose an agent above the conversation; the destination and model are visible before sending. A change of agent starts a fresh conversation and does not forward the earlier agent's transcript.

The settings follow the [local model](https://zed.dev/docs/ai/use-a-local-model) and [OpenAI-compatible provider](https://zed.dev/docs/ai/llm-providers) conventions. Supported provider keys include `ollama`, `lmstudio`, `llama.cpp`, `openai` and named `openai_compatible` entries. Ollama uses its native chat API; the others use Chat Completions. For a server with a bearer key, set the provider's `api_key_env` to the **name** of an environment variable. Use HTTPS for authenticated remote servers. A remote endpoint receives the context you send. Reload after changing credentials; conversations are partitioned by the configured agent, model, destination and credential identity. A workbook model needs function calling for tools; a text-only model can still discuss explicitly attached cells.

### Context limit

**Context limit** sets the token budget for this model in the panel. Automatic uses the configured window, capped by any smaller window reported by **Find models**. An endpoint that reports no limit and has no configured window shows **Local estimate**, not a claim about model capacity. The budget reserves room for the reply. Changing it affects conversation compaction, not server settings. For a permanent choice, set the provider's `context_window` or the model's `max_tokens` in `available_models`; `max_output_tokens` is a separate reply limit.

Wingman summarizes earlier exchanges when the estimated request needs more room. The same model receives a separate text-only request, without tools. Its handoff and recent complete exchanges are used for the next answer; the current request stays intact. The panel keeps a separate in-memory scrollback of up to 160 entries, trimming older entries above 512 KiB. A rejected context gets one smaller retry. Use `/compact` to summarize early. Stopping a summary or losing the connection leaves the original context in place.

While composing, **Ctrl+O** expands or collapses tool receipts, including summary details. **Ctrl+T** shows or hides thinking returned by the provider. Both change the display only; neither changes reasoning effort or repeats an action.

For a vision-capable model, set `"snap_tool_results": true` in its `available_models` entry to compact long command, function and chart-recommendation lists. Wingman compares the original receipt, a compact table and an image of the descriptions. It sends an image only when that beats both text forms by the estimated token cost. Identifiers, arguments and cursors stay as text; cell values and formulas stay verbatim. This option defaults off.

### Inspect a picture or chart

For a model that accepts both images and tools, set `"capabilities": { "tools": true, "image": true }` in its `available_models` entry and reload. Model discovery alone does not enable image sharing. Select an embedded picture, a single cell containing a picture, or a chart and ask Wingman to inspect it. You can also ask it to find a chart by title: it can list charts and choose one by ID without changing your selection.

**Send image** approves the named preview for the configured model, including a retry of that request. With **Always allow** in a write-enabled mode, that preview can be sent without another prompt. Only the picture or rendered chart is attached—not a desktop screenshot, a file path, or the workbook. Chart previews support bounded cell references and constant arrays; for calculated or indirect sources, use `inspect_chart` to read the definition instead. The chat keeps the inspection receipt, not the pixels; ask for another inspection if the model needs to look again.

### Thinking modes

**Find models → choose a model → Thinking** works without editing JSON. Thinking sits beside **Access**, or directly below it when the dock is narrow. Leave **Server default** selected to send no override. Model discovery identifies the model; it does not prove which reasoning levels the server implements.

For Chat Completions, the manual choices are none, minimal, low, medium, high, xhigh and max. Select a level your endpoint accepts; `max` and `xhigh` are different values. Native Ollama uses `think` rather than `reasoning_effort` and offers on/off and low/medium/high. To restrict the list, put e.g. `"reasoning_effort_levels": ["low", "medium", "high"], "reasoning_effort": "medium"` in the model's `available_models` entry. A comma-separated list also works. An explicit `"supports_thinking": false` disables the control. An agent can inherit or override the configured default using `effort`.

Wingman runs its own tool loop against the configured model API. In Zed these are [model providers](https://zed.dev/docs/ai/llm-providers); [external agents](https://zed.dev/docs/ai/external-agents) are separate programs using ACP, with their own sessions, permissions and tools. Wingman does not launch Zed's `agent_servers`. Use `language_models` for a model endpoint and `context_servers` for additional MCP tools.

## Work with a sheet

| Access | What the model can do |
| --- | --- |
| Chat only | Answer from the conversation and any context you explicitly attach. No workbook tools. |
| Read only (default) | Inspect workbook structure, cell values, formulas, formatting, comments, charts, tables, names and native scripts. |
| Read / write | Change cells, formulas, formatting, charts, tables, comments and native scripts. By default, review the arguments and choose **Apply edit**. |
| Read / write / MCP | The same workbook permissions, plus tools from servers you explicitly connect. By default, review the destination, tool and arguments and choose **Allow call**. |

**Approval** starts at **Ask every time**. Choose **Always allow** to skip confirmations for actions already permitted in this thread and access mode. In MCP mode that includes connected external tools; workbook Undo cannot reverse their actions. Changing thread, agent or access mode, reloading settings, or closing the pane restores **Ask every time**.

**Tool rounds** defaults to **12**. A round is one exchange with the model, which can request several tools together. A finite budget reserves its last exchange for the answer. **Unlimited** removes the round count limit; request-size limits and **Stop** still apply. If the provider finishes without a final answer, Wingman asks once more for text using the results already received. That follow-up cannot run more tools.

The initial access setting accepts `ask`, `write`, `write_mcp`, or `minimal` in `agent.default_profile`; an individual agent can override it. Type `@` for references. Arrow keys select a completion; Tab or Enter accepts it, Escape dismisses it. For example:

- `Why does @read D3:D12 return different margins from @selection?`
- `Which cells contain XYZ anywhere in this workbook? Give me the sheet names and addresses.`
- `Find formulas using XLOOKUP on the Orders sheet, then explain what the first match depends on.`
- `Compare @read 'Q1 Sales'!B2:D8 with @read 'Q2 Sales'!B2:D8.`
- `Add a column chart for @read A1:B12 and give it the title “Weekly sales”.`
- `Summarize @comments, then propose a reply for the active cell.`
- `Sort @read A1:E30 by column C descending and keep the header row.`
- `In @editor, move the comments in column AB into the same rows in AC. Center a header in AC1 that says xyz.`

With the worksheet open, `@editor` refers to that sheet; in the native Macro Editor it refers to the open draft. In **Read / write**, the column transfer writes the full note or thread locally, even when the model only saw a short preview. **Copy** keeps the comments in AB; **move** also removes them. By default AB9 goes to AC9. If AB1 also has a comment, ask to start the output at AC2 so AC1 can hold the header without losing the first note. Rows without comments stay untouched. Occupied output cells cause the whole transfer to stop. Ctrl+Z reverses the transfer and header together.

References capture workbook data when you send. The selected endpoint receives the message, those references, and any subsequent workbook-tool results. **Deny** refuses the proposed edit and ends the turn, including any queued calls. **Stop** cancels a reply in progress. Neither undoes an earlier approved edit. If the sheet, selection or workbook revision changes before a tool runs, Wingman stops rather than operating on a new target. Changes go through GridCraft's own commands and undo history.

Enter sends; Shift+Enter adds a line. `/mode chat|read|write|mcp` changes access; `/settings` opens JSON; `/help` lists commands. `/new` starts a conversation and keeps the previous one under **Threads** for that agent. `/clear` discards the current conversation. Threads are held in memory, not in the spreadsheet. The desktop control command `view.wingman` toggles the dock or accepts `{"on":true}` / `{"on":false}`.

To reuse the comment extraction in another run, open a native macro for editing and ask Wingman to add the same column transfer as a step. Approving the draft does not move anything; the command reads the sheet's then-current comments when you run the saved macro.

### Search and formula inspection

Wingman’s `grep` searches populated cells across visible sheets by default. It can narrow the search to a sheet, a range or a column, and match literal text or a regular expression in values, formatted display text, formulas, or comment threads. It returns sheet names, addresses and matching excerpts. Large searches yield to the UI between slices; **Stop** cancels them. Only result pages go to the model. No cursor means the search is finished; a skipped-fields count distinguishes an incomplete scan from a clean miss.

Use `check_formula` before entering a proposed formula: it checks the native cell-entry rules and reports static warnings without writing or evaluating it. Use `formula_diagnostics` after an edit to find cached spreadsheet errors: text reading `#REF!` is not a reference error. `inspect_formula` traces direct static references and reports incomplete traces. `profile_range` shows stored value types, blanks and numeric-looking text by column; it does not reinterpret dates or currency as separate storage types. None guarantees that the formula answers the intended business question.

For example, if A2 contains the literal text `pending`, checking `=A2+1` can report a cached operand warning at A2. A stored numeric string that GridCraft can coerce is different. A direct zero divisor is reported separately. These checks do not evaluate conditional branches, expand ranges, or follow indirect references.

### Working across ribbon tabs

Wingman starts with workbook search and inspection. Asking it to open **Home**, **Data**, or another tab changes the tools offered to the model on its next request; your ribbon stays put. It can browse a tab’s groups, narrow to one group, or use `find_commands` to look up an individual button. You can browse the same entries in the dock’s **Ribbon guide** without connecting a model. [Compare the agent workflows](wingman-parity.md) or [browse the complete control index](wingman-commands.md).

For example: “On Orders, format B2:C200 as currency. Put the sum of each row in D, then fill it down.” After opening Home, Wingman can request one reviewed plan:

1. Apply the Currency number format to `B2:C200`.
2. Set `D2` to `=B2+C2`.
3. Drag-fill `D2` into `D2:D200`. The last formula references B200 and C200.

The plan displays its ordered targets and exact arguments. Up to 16 eligible changes can share one approval and one undo step. Each step uses the selection left by the preceding step; a failed step rolls back the entire plan. Otherwise, individual approvals remain available. Host actions, scripts, and external calls cannot be hidden in a workbook edit plan.

To find an object rather than a cell, ask for a chart by title, a table by name, or a defined name. `list_objects` can filter the native object list before paging; its chart IDs and table ranges can be passed to other workbook tools.

### Undo and the Macro Editor

Each approved sheet edit is one step in GridCraft’s own undo history, labeled **Wingman**. Use Ctrl+Z (Cmd+Z on macOS) to undo the latest edit in chronological order; use the normal Redo command to restore it. A formula fill or rectangular paste is a single step. Typing in a draft still uses normal text undo.

In **Automate → Macros → Edit**, open **Wingman** and attach `@macro`. In a write mode the agent can propose changes to the open native draft, including inserting or replacing steps inside a nested list. If you change the draft, an older proposal can’t overwrite it. Undo draft edits inside the editor; **Save** commits the macro to the workbook. Wingman never runs the macro. Imported Excel VBA modules are different: GridCraft preserves their binary project unchanged.

For example, ask “Change step 1’s condition to `A1 > 10` and keep both branches.” Wingman can edit an If condition, a Repeat range and limit, or a Stop message and rollback flag in place. Notes, breakpoints and enabled flags are editable separately. To build a branch, add an empty If or Repeat, then insert steps into its nested list.

**Updating a column with Excel VBA:** Select a few representative rows, choose **Chat only**, and ask “Using `@selection`, draft a VBA Sub that fills blank cells in column D from column C on the Orders sheet, skipping the header. Return one VBA code block.” If revising an existing module, paste only the source you want to share with the selected model. **Copy code** copies that block without the surrounding explanation; **Copy** copies the whole reply, including a plain-text VBA reply. Review and paste the code into the intended module in Excel’s Visual Basic Editor, then save the workbook as `.xlsm`. Nothing is installed or run by copying.

For the same update entirely in GridCraft, open or create a native macro under **Automate → Macros**. In its editor, choose **Read / write**, attach `@macro`, and ask for the equivalent Repeat and If steps. For example, bind the input to C:D’s data rows: inside a row Repeat, `Row.cell(1,2)` is that row’s D cell, and the formula `={{Row.cell(1,1)}}` links it to C on the same row. The reference is resolved when the macro runs, not when the model proposes the step. Review each edit, **Save** the draft, and use **Run Macro** yourself when ready.

### Connect external tools

Add [Zed-style MCP servers](https://zed.dev/docs/ai/mcp) at the top level of `wingman.json`. For example, after [Localflame](https://github.com/CommanderTurtle/localflame) is installed and its Firecrawl service is available:

```json
{
  "context_servers": {
    "localflame": {
      "command": "bun",
      "args": ["/absolute/path/to/localflame/bin/localflame.js"],
      "allowed_tools": ["firecrawl_search", "firecrawl_outline", "firecrawl_read"],
      "env": { "FIRECRAWL_API_URL": "http://127.0.0.1:3002" }
    }
  }
}
```

Choose **Reload → Read / write / MCP → Connect**. Open **Tools** to inspect or refresh the server’s catalog without restarting it. Ask for a search, review the outgoing query, then approve it. A later read can use the resource returned by the search: the same server process stays alive. Disconnecting clears process-local state. Leaving MCP mode, changing agents, starting a new thread or closing the panel also disconnects. Changing modes does not erase messages already in the conversation.

For a Streamable HTTP server use `url` instead of `command`/`args`; HTTPS is required except on loopback. `headers` sets fixed request headers; `header_env` maps header names to environment variables containing the complete header value. OAuth login, MCP prompts, sampling and binary result attachments are outside this integration. `allowed_tools` optionally limits what the model sees; `timeout_seconds` defaults to 120. A local server runs with the user’s OS permissions: configure only trusted programs. GridCraft can undo workbook edits, not actions performed on another server.
