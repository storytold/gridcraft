# Test Runner Scripts

This directory contains scripts for running and managing tests for the GridCraft MCP Python client.

## Available Scripts

### `run_tests.sh`

Main test runner script that provides a convenient interface for running all types of tests.

#### Usage

```bash
./scripts/run_tests.sh              # Run all tests (default)
./scripts/run_tests.sh unit         # Run only unit tests
./scripts/run_tests.sh integration  # Run only integration tests
./scripts/run_tests.sh examples     # Run only example validation
./scripts/run_tests.sh all          # Run all tests
./scripts/run_tests.sh coverage     # Run tests with coverage report
./scripts/run_tests.sh clean        # Clean test artifacts
```

#### Features

- ✅ **Automatic dependency checking** - Verifies Python and pytest are installed
- ✅ **Environment validation** - Checks for GridCraft CLI for integration tests
- ✅ **Color-coded output** - Easy to read results
- ✅ **Comprehensive test coverage** - Runs unit, integration, and example tests
- ✅ **Coverage reporting** - Generates HTML and terminal coverage reports
- ✅ **Cleanup functionality** - Removes test artifacts
- ✅ **User-friendly interface** - Clear instructions and summaries

#### Requirements

- Python 3.8+
- pip
- pytest (installed automatically if missing)
- GridCraft CLI with MCP server (for integration tests)

#### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `GRIDCRAFT_CLI_PATH` | Path to gridcraft-cli executable | `gridcraft-cli` |
| `GRIDCRAFT_PORT` | Port for MCP server | `7979` |

#### Example Output

```
╔════════════════════════════════════════════════════════════╗
║     GridCraft MCP Client - Automated Test Runner          ║
╚════════════════════════════════════════════════════════════╝

[INFO] Checking dependencies...
[✓] Dependencies check passed
[INFO] Running ALL tests...
[INFO] Running unit tests...
============================= test session starts ==============================
...
collected 30 items

tests/test_client.py::TestGridCraftClient::test_client_initialization PASSED [  3%]
tests/test_client.py::TestGridCraftClient::test_read_range PASSED        [  6%]
...

tests/test_examples.py::test_basic_usage_example_syntax PASSED          [100%]

======================== 35 passed in 0.45s ===========================
[✓] Unit tests completed successfully
[INFO] Running example validation tests...
...
[✓] Example validation tests completed successfully
[✓] All tests completed successfully!

╔════════════════════════════════════════════════════════════╗
║           GridCraft MCP Client - Test Summary            ║
╚════════════════════════════════════════════════════════════╝

📊 Test Coverage:
   • Unit Tests: 30+ tests (client initialization, methods, error handling)
   • Integration Tests: 25+ tests (real GridCraft interaction)
   • Example Tests: 5+ tests (syntax validation)
   • Total: 60+ tests
```

## Test Types

### Unit Tests

Run with: `./scripts/run_tests.sh unit`

Tests individual components using mocks:
- Client initialization
- Error handling
- Request sending
- All public methods
- Context manager

### Integration Tests

Run with: `./scripts/run_tests.sh integration`

Tests real interaction with GridCraft MCP server:
- File operations (open, save, export)
- Data operations (read, write, formulas)
- Chart and table creation
- Sheet management
- Clipboard operations

**Requires:** GridCraft CLI with MCP server

### Example Tests

Run with: `./scripts/run_tests.sh examples`

Validates that all example scripts are valid:
- Python syntax validation
- Import validation
- File existence checks

### Coverage Tests

Run with: `./scripts/run_tests.sh coverage`

Runs all tests with coverage reporting:
- Terminal coverage report
- HTML coverage report (saved to `htmlcov/index.html`)
- Shows which lines are covered by tests

## CI/CD Integration

The test runner is designed to work with CI/CD pipelines. Example GitHub Actions workflow:

```yaml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.10'
      - run: chmod +x scripts/run_tests.sh
      - run: ./scripts/run_tests.sh all
```

## Best Practices

1. **Run tests before committing** - Ensure all tests pass
2. **Check coverage regularly** - Maintain high test coverage
3. **Test in different environments** - Different Python versions, OS
4. **Run integration tests** - When GridCraft CLI is available
5. **Clean artifacts** - Remove test files after use

## Troubleshooting

### Common Issues

**Issue: Python not found**
```bash
# Install Python 3.8+
# Ubuntu/Debian: sudo apt install python3 python3-pip
# macOS: brew install python
# Windows: Download from python.org
```

**Issue: pytest not found**
```bash
# Install pytest
pip install pytest pytest-cov
```

**Issue: GridCraft CLI not found**
```bash
# Install GridCraft CLI
# Build from source or download from releases
# Set GRIDCRAFT_CLI_PATH environment variable
export GRIDCRAFT_CLI_PATH=/path/to/gridcraft-cli
```

**Issue: Tests failing**
```bash
# Run with verbose output
./scripts/run_tests.sh all -v

# Check logs
# GridCraft logs are in:
# Linux: ~/.config/gridcraft/logs/
# macOS: ~/Library/Application Support/GridCraft/logs/
# Windows: %APPDATA%\GridCraft\logs\
```

## Additional Tools

### Manual Test Commands

```bash
# Run specific test file
pytest tests/test_client.py -v

# Run with coverage
pytest tests/ --cov=gridcraft_mcp --cov-report=term-missing

# Run specific test class
pytest tests/test_client.py::TestGridCraftClient -v

# Run specific test method
pytest tests/test_client.py::TestGridCraftClient::test_read_range -v

# Generate coverage HTML report
pytest tests/ --cov=gridcraft_mcp --cov-report=html
```

### Test Environment Setup

```bash
# Install development dependencies
pip install -e ".[dev]"

# Run linters
black gridcraft_mcp
flake8 gridcraft_mcp
mypy gridcraft_mcp

# Run all CI checks
./scripts/run_tests.sh all
```

## Support

For issues with the test runner:
1. Check the output for error messages
2. Run with verbose mode: `./scripts/run_tests.sh all -v`
3. Check GridCraft logs
4. Review test files in `tests/` directory
5. Open an issue on GitHub

## License

The test runner scripts are part of the GridCraft MCP Python client and are licensed under either:
- MIT License
- Apache License, Version 2.0

at your option.
