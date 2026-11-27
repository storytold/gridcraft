# Wingman

Choose the **Wingman** tile at the far right of the desktop ribbon, or type **Wingman** in command search. The tile stays in place when the ribbon groups scroll and becomes a compact button when the ribbon collapses. Drag the panel's left edge to resize it. Each workbook window has its own threads and permissions. Opening the panel does not contact a model.

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

The settings follow the [local model](https://zed.dev/docs/ai/use-a-local-model) and [OpenAI-compatible provider](https://zed.dev/docs/ai/llm-providers) conventions. Supported provider keys include `ollama`, `lmstudio`, `llama.cpp`, `openai` and named `openai_compatible` entries. Ollama uses its native chat API; the others use Chat Completions. For a server with a bearer key, set the provider's `api_key_env` to the **name** of an environment variable. Use HTTPS for authenticated remote servers. A workbook model needs function calling for tools; a text-only model can still discuss explicitly attached cells.

### Thinking modes

For a model that supports adjustable reasoning, add `"reasoning_effort_levels": ["low", "medium", "high"], "reasoning_effort": "medium"` to its entry in `available_models`. A comma-separated list also works. **Thinking** selects the level for the next turn; **Server default** omits the override. Only advertise levels your model accepts. A native Ollama model with `"supports_thinking": true` can instead offer on/off. Defaults can also use `effort` and `enable_thinking` on `agent.default_model` or a named Wingman agent.

## Work with a sheet

| Access | What the model can do |
| --- | --- |
| Chat only | Answer from the conversation and any context you explicitly attach. No workbook tools. |
| Read only (default) | Inspect workbook structure, cell values, formulas, formatting, comments, charts, tables, names and native scripts. |
| Read / write | Propose changes to cells, formulas, formatting, charts, tables, comments and native scripts. Sort or filter a range, define a name, resize rows or columns, or add data bars. Each edit shows its arguments and requires **Apply edit**. |
| Read / write / MCP | The same workbook permissions, plus tools from servers you explicitly connect. Each external call shows the destination, tool and arguments and requires **Allow call**. |

The initial access setting accepts `ask`, `write`, `write_mcp`, or `minimal` in `agent.default_profile`; an individual agent can override it. Type `@` for references. Arrow keys select a completion; Tab or Enter accepts it, Escape dismisses it. For example:

- `Why does @read D3:D12 return different margins from @selection?`
- `Compare @read 'Q1 Sales'!B2:D8 with @read 'Q2 Sales'!B2:D8.`
- `Add a column chart for @read A1:B12 and give it the title “Weekly sales”.`
- `Summarize @comments, then propose a reply for the active cell.`
- `Sort @read A1:E30 by column C descending and keep the header row.`
- `In @editor, move the comments in column AB into the same rows in AC. Center a header in AC1 that says xyz.`

With the worksheet open, `@editor` refers to that sheet; in the native Macro Editor it refers to the open draft. In **Read / write**, the column transfer writes the full note or thread locally, even when the model only saw a short preview. **Copy** keeps the comments in AB; **move** also removes them. By default AB9 goes to AC9. If AB1 also has a comment, ask to start the output at AC2 so AC1 can hold the header without losing the first note. Rows without comments stay untouched. Occupied output cells cause the whole transfer to stop. Ctrl+Z reverses the transfer and header together.

References capture workbook data when you send. The selected endpoint receives the message, those references, and any subsequent workbook-tool results. **Deny** refuses the proposed edit and ends the turn, including any queued calls. **Stop** cancels a reply in progress. Neither undoes an earlier approved edit. If the sheet, selection or workbook revision changes before a tool runs, Wingman stops rather than operating on a new target. Changes go through GridCraft's own commands and undo history.

Enter sends; Shift+Enter adds a line. `/mode chat|read|write|mcp` changes access; `/settings` opens JSON; `/help` lists commands. `/new` starts a conversation and keeps the previous one under **Threads** for that agent. `/clear` discards the current conversation. Threads are held in memory, not in the spreadsheet. The desktop control command `view.wingman` toggles the dock or accepts `{"on":true}` / `{"on":false}`.

To reuse the comment extraction in another run, open a native macro for editing and ask Wingman to add the same column transfer as a step. Approving the draft does not move anything; the command reads the sheet's then-current comments when you run the saved macro.

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
