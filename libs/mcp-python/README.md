# GridCraft MCP Python Client

A Python client for the [GridCraft](https://github.com/storytold/gridcraft) control channel.
It drives a running GridCraft window over the JSON-lines TCP protocol, so scripts, notebooks
and agents can create workbooks, write data, evaluate formulas and insert charts.

## Installation

From the repository root:

```sh
pip install -e libs/mcp-python
```

For development extras (pytest, coverage, linters):

```sh
pip install -e "libs/mcp-python[dev]"
```

## Quick start

Start GridCraft with the control channel in one terminal:

```sh
cargo run --release -p gridcraft -- --sample sales --control 7979
```

Then use the client:

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient(port=7979) as client:
    client.new_workbook()
    client.set_cell("A1", "Product")
    client.set_cell("B1", 100)
    print(client.read_range("A1:B1"))
```

## Documentation

- [Usage examples](docs/usage_examples.md) — the full API with runnable snippets
- [Testing](docs/testing.md) — test suite, coverage and CI
- [Development](docs/development.md) — contributing and project layout

## Examples

Runnable scripts live in [`examples/`](examples):

```sh
python3 examples/basic_usage.py
python3 examples/data_analysis.py
python3 examples/charts_and_tables.py
python3 examples/automation.py
```

## Tests

```sh
./scripts/run_tests.sh            # unit + example tests
./scripts/run_tests.sh coverage   # with a coverage report
./scripts/run_ci.sh               # tests + lint + format + type check
```

Integration tests need `gridcraft-cli` on `PATH`; see [docs/testing.md](docs/testing.md).

## Licence

Dual-licensed under MIT or Apache-2.0, like the rest of GridCraft.
