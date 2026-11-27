# Wingman

Choose the **Wingman** tile at the far right of the desktop ribbon, or type **Wingman** in command search. The tile stays in place when the ribbon groups scroll and becomes a compact button when the ribbon collapses. Drag the panel's left edge to resize it. Each workbook window has its own threads and permissions. Opening the panel does not contact a model.

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

## Work with a sheet

| Access | What the model can do |
| --- | --- |
| Chat only | Answer from the conversation and any context you explicitly attach. No workbook tools. |
| Read only (default) | Inspect workbook structure, cell values, formulas, formatting, comments, charts, tables, names and native scripts. |
| Read / write | Propose changes to cells, formulas, formatting, charts, tables, comments and native scripts. Sort or filter a range, define a name, resize rows or columns, or add data bars. Each edit shows its arguments and requires **Apply edit**. |

The initial access setting accepts `ask`, `write`, or `minimal` in `agent.default_profile`; an individual agent can override it. Type `@` for references. Arrow keys select a completion; Tab or Enter accepts it, Escape dismisses it. For example:

- `Why does @read D3:D12 return different margins from @selection?`
- `Compare @read 'Q1 Sales'!B2:D8 with @read 'Q2 Sales'!B2:D8.`
- `Add a column chart for @read A1:B12 and give it the title “Weekly sales”.`
- `Summarize @comments, then propose a reply for the active cell.`
- `Sort @read A1:E30 by column C descending and keep the header row.`

References capture workbook data when you send. The selected endpoint receives the message, those references, and any subsequent workbook-tool results. **Deny** refuses the proposed edit and ends the turn, including any queued calls. **Stop** cancels a reply in progress. Neither undoes an earlier approved edit. If the sheet, selection or workbook revision changes before a tool runs, Wingman stops rather than operating on a new target. Changes go through GridCraft's own commands and undo history.

Enter sends; Shift+Enter adds a line. `/mode chat|read|write` changes access; `/settings` opens JSON; `/help` lists commands. `/new` starts a conversation and keeps the previous one under **Threads** for that agent. `/clear` discards the current conversation. Threads are held in memory, not in the spreadsheet. The desktop control command `view.wingman` toggles the dock or accepts `{"on":true}` / `{"on":false}`.

GridCraft's native Automate scripts are command lists. Imported Excel VBA projects are preserved binary data: Wingman can describe the metadata or help rewrite VBA you paste in chat; it cannot patch an embedded VBA module. Workbook tools cannot execute macros, open arbitrary files, run shell commands, or contact other services.
