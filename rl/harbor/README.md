# GridCraft RL environments (Harbor)

[Harbor](https://docs.harborframework.com) tasks that have an agent work on spreadsheets with
GridCraft inside Docker. Each task is a directory Harbor can run directly.

```
rl/harbor/
├── build-base.sh                     # builds gridcraft-harbor-base:latest from this checkout
├── docker/gridcraft.Dockerfile       # Rust build → Ubuntu 24.04 + gridcraft-cli + python3
└── tasks/
    └── sales-average-column/         # example task
        ├── task.toml                 # timeouts, resources, optional MCP server
        ├── instruction.md            # what the agent is told
        ├── environment/Dockerfile    # FROM the base image; generates /app/sales.xlsx
        ├── solution/solve.sh         # oracle solution
        └── tests/                    # verifier → /logs/verifier/reward.txt
```

## Setup

```sh
uv tool install harbor               # Docker must be running
rl/harbor/build-base.sh              # rebuild after changing GridCraft code
```

## Run

```sh
cd rl/harbor
harbor run -p tasks/sales-average-column -a oracle            # must score 1.0
harbor run -p tasks/sales-average-column -a claude-code -m anthropic/claude-sonnet-5-5
harbor run -p tasks -a claude-code -m anthropic/claude-sonnet-5-5   # every task
harbor task start-env -p tasks/sales-average-column -e docker -a -i  # poke around inside
harbor view ./jobs
```

## Adding a task

Copy `tasks/sales-average-column` and change:

- **environment/Dockerfile**: build the starting workbook in code (`gridcraft-cli run --sample …`
  or `--cmd`/`--script` steps). Don't commit `.xlsx` fixtures.
- **instruction.md**: the goal, the file paths, and what has to be saved where.
- **tests/test_outputs.py**: `gates()` must all pass or the reward is 0. The reward is the fraction
  of `checks()` passed. Hardcode expected values; don't derive them from the agent's file.
- **solution/solve.sh**: must reach reward 1.0 with `-a oracle`.

The agent drives GridCraft with `gridcraft-cli` (`commands`, `run`, `cat`, `eval`). To use
GridCraft as an MCP tool instead, uncomment `[[environment.mcp_servers]]` in `task.toml`.

The verifier runs `gridcraft-cli` inside the same container the agent used, so an agent could
tamper with the binary. For training at scale, move verification to a separate environment
(`[verifier.environment]`, see Harbor's "Separate verifier" docs).
