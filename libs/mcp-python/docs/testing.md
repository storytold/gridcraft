# Testing Guide

This document provides comprehensive information about testing the GridCraft MCP Python client.

## Table of Contents

- [Test Types](#test-types)
- [Running Tests](#running-tests)
- [Test Organization](#test-organization)
- [Writing Tests](#writing-tests)
- [Integration Tests](#integration-tests)
- [Test Coverage](#test-coverage)
- [Continuous Integration](#continuous-integration)
- [Best Practices](#best-practices)

---

## Test Types

### Unit Tests

**Location:** `tests/test_client.py`

**Purpose:** Test individual components and methods in isolation using mocks.

**Coverage:**
- Client initialization
- Error handling
- Request sending
- All public methods
- Context manager

**Example:**
```python
@patch('subprocess.Popen')
@patch('socket.socket')
def test_start_success(self, mock_socket, mock_popen):
    """Test successful client start"""
    # Setup mocks
    # Execute
    # Assert
```

### Integration Tests

**Location:** `tests/test_integration.py`

**Purpose:** Test real interaction with GridCraft MCP server.

**Coverage:**
- File operations (open, save, export)
- Data operations (read, write, formulas)
- Chart and table creation
- Sheet management
- Clipboard operations
- Performance with real data

**Requirements:**
- GridCraft CLI with MCP server
- GridCraft must be in PATH or specified via `GRIDCRAFT_CLI_PATH`

### Example Tests

**Location:** `tests/test_examples.py`

**Purpose:** Verify that example scripts are valid and run without errors.

**Coverage:**
- All example scripts in `examples/` directory
- Python syntax validation
- Import validation

---

## Running Tests

### Prerequisites

```bash
# Install development dependencies
pip install -e ".[dev]"

# Ensure GridCraft CLI is available
# Either in PATH as 'gridcraft-cli'
# or set GRIDCRAFT_CLI_PATH environment variable
```

### Running Unit Tests

```bash
# Run all unit tests
pytest tests/test_client.py -v

# Run with coverage
pytest tests/test_client.py --cov=gridcraft_mcp --cov-report=html

# Run specific test class
pytest tests/test_client.py::TestGridCraftClient -v

# Run specific test method
pytest tests/test_client.py::TestGridCraftClient::test_client_initialization -v
```

### Running Integration Tests

```bash
# Run integration tests (requires GridCraft CLI)
pytest tests/test_integration.py -v

# Run with verbose output
pytest tests/test_integration.py -v -s

# Run specific integration test
pytest tests/test_integration.py::TestIntegration::test_create_and_read_workbook -v

# Run performance tests
pytest tests/test_integration.py::TestPerformance -v
```

### Running All Tests

```bash
# Run all tests (unit + integration)
pytest tests/ -v

# Run all tests with coverage
pytest tests/ --cov=gridcraft_mcp --cov-report=term-missing

# Run all tests and generate HTML coverage report
pytest tests/ --cov=gridcraft_mcp --cov-report=html
# Coverage report will be in htmlcov/index.html
```

### Running Example Tests

```bash
# Run example validation tests
pytest tests/test_examples.py -v
```

---

## Test Organization

```
tests/
├── __init__.py                    # Test package initialization
├── conftest.py                    # Pytest fixtures and configuration
├── test_client.py                 # Unit tests (30+ tests)
├── test_integration.py            # Integration tests (25+ tests)
└── test_examples.py               # Example validation tests
```

### Test Files Structure

#### `conftest.py`

Contains:
- Custom pytest fixtures
- Pytest configuration
- Test data generators

**Fixtures:**
- `sample_data()` - Provides sample data for testing
- `empty_client()` - Provides empty GridCraftClient instance

#### `test_client.py`

**Classes:**
- `TestGridCraftClient` - Tests for client initialization and core functionality
- `TestExceptions` - Tests for custom exception classes

**Test Categories:**
1. Initialization tests
2. Start/stop tests
3. Request sending tests
4. Method tests (all 40+ public methods)
5. Context manager tests

#### `test_integration.py`

**Classes:**
- `TestIntegration` - Basic integration tests
- `TestPerformance` - Performance tests
- `TestRealWorldScenarios` - Real-world usage scenarios

**Test Categories:**
1. Connection tests
2. File operations
3. Data operations
4. Chart and table tests
5. Sheet management
6. Clipboard operations
7. Error handling
8. Performance tests
9. Real-world scenarios

#### `test_examples.py`

**Purpose:** Validate that all example scripts are valid.

**Tests:**
- Syntax validation
- Import validation
- File existence checks

---

## Writing Tests

### Test Structure

```python
import pytest
from unittest.mock import patch, MagicMock
from gridcraft_mcp import GridCraftClient, ConnectionError, CommandError


class TestFeatureName:
    """Test suite for specific feature"""
    
    def test_feature_name_scenario(self):
        """Clear description of what is being tested"""
        # Setup
        client = GridCraftClient()
        
        # Mock dependencies if needed
        with patch('module.function') as mock_function:
            mock_function.return_value = "expected"
            
            # Execute
            result = client.method()
            
            # Assert
            assert result == "expected"
            mock_function.assert_called_once()
```

### Unit Test Template

```python
@patch('module.to_mock')
def test_method_name(self, mock_module):
    """Test method_name with specific parameters"""
    # Setup
    client = GridCraftClient()
    mock_module.return_value = "mocked_result"
    
    # Execute
    result = client.method_name(param1="value1", param2=42)
    
    # Assert
    assert result == "expected_result"
    mock_module.assert_called_once_with(expected_args)
```

### Integration Test Template

```python
@pytest.mark.skipif(not gridcraft_available, reason="GridCraft CLI not available")
def test_real_world_operation(self, gridcraft_client):
    """Test real-world operation with GridCraft"""
    client = gridcraft_client
    
    # Setup data
    client.set_cell("A1", "Test")
    
    # Execute operation
    result = client.read_range("A1:A1")
    
    # Assert
    assert result == [["Test"]]
```

### Best Practices for Writing Tests

1. **Descriptive names:** Use clear test method names
   - ✅ `test_read_range_returns_correct_data`
   - ❌ `test_read_range`

2. **Single responsibility:** Each test should test one thing

3. **Clear setup/execute/assert sections:**
   ```python
   # Setup
   client = GridCraftClient()
   
   # Execute
   result = client.method()
   
   # Assert
   assert result == expected
   ```

4. **Use mocks for external dependencies:**
   ```python
   @patch('requests.post')
   def test_method(self, mock_post):
       # Mock HTTP requests
   ```

5. **Test both success and error cases:**
   ```python
   def test_error_handling(self):
       # Test success path
       # Test error path
   ```

6. **Test edge cases:**
   - Empty data
   - Large data
   - Invalid parameters
   - Network errors

7. **Keep tests fast:** Avoid long-running operations in unit tests

8. **Use fixtures for common setup:**
   ```python
   @pytest.fixture
def clean_workbook(self, gridcraft_client):
       client = gridcraft_client
       client.new_workbook()
       yield client
   ```

---

## Integration Tests

### Requirements

Integration tests require:
1. GridCraft CLI with MCP server support
2. GridCraft must be executable
3. MCP server must be accessible on port 7979 (default)

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `GRIDCRAFT_CLI_PATH` | Path to gridcraft-cli executable | `gridcraft-cli` |
| `GRIDCRAFT_PORT` | Port for MCP server | `7979` |

### Skipping Integration Tests

By default, integration tests are skipped if GridCraft is not available.

To run integration tests:

```bash
# Set path to GridCraft CLI
export GRIDCRAFT_CLI_PATH=/path/to/gridcraft-cli

# Run integration tests
pytest tests/test_integration.py -v
```

### Integration Test Categories

#### Basic Operations
- Client connection
- Workbook creation
- Data reading/writing
- Formula evaluation

#### Advanced Features
- Chart creation
- Table creation
- Sorting
- Filtering
- Conditional formatting

#### File Operations
- Open/save workbooks
- Export to CSV
- Export to PDF (if supported)

#### Sheet Management
- Add/delete/rename sheets
- Working with multiple sheets

#### Real-World Scenarios
- Budget spreadsheet
- Sales analysis
- Data dashboard

---

## Test Coverage

### Current Coverage

```
Unit Tests: 30+ tests
Integration Tests: 25+ tests
Example Tests: 5+ tests
Total: 60+ tests
```

### Coverage Metrics

To check test coverage:

```bash
# Install coverage if not already installed
pip install pytest-cov

# Run tests with coverage
pytest tests/ --cov=gridcraft_mcp --cov-report=term-missing

# Generate HTML report
pytest tests/ --cov=gridcraft_mcp --cov-report=html

# Open coverage report
xdg-open htmlcov/index.html  # Linux
open htmlcov/index.html       # macOS
start htmlcov/index.html      # Windows
```

### Target Coverage

- **Unit tests:** 100% coverage of public API
- **Integration tests:** 80%+ coverage of real-world scenarios
- **Example tests:** 100% validation of examples

### Coverage Reports

Coverage reports show:
- Which lines are covered by tests
- Which lines are missing coverage
- Branch coverage
- Function coverage

**Example output:**
```
gridcraft_mcp/client.py         95%   10/200 lines
gridcraft_mcp/exceptions.py    100%    0/0 lines
tests/test_client.py            98%   50/51 lines
```

---

## Continuous Integration

### GitHub Actions

The project includes GitHub Actions workflows for:
- Running unit tests on every push
- Running integration tests on schedule
- Checking code quality
- Running coverage reports

### CI Workflow Files

#### `.github/workflows/ci.yml`

Runs on:
- Push to main branch
- Pull requests
- Schedule (daily)

**Jobs:**
1. **Test:** Runs unit tests with coverage
2. **Lint:** Runs linters (flake8, black check)
3. **Type Check:** Runs mypy type checking
4. **Format:** Checks code formatting

#### Running CI Locally

```bash
# Run all CI checks locally
./scripts/run_ci.sh

# Or manually:
# Format check
black --check gridcraft_mcp

# Lint check
flake8 gridcraft_mcp

# Type check
mypy gridcraft_mcp

# Test
pytest tests/test_client.py
```

### CI Configuration

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [ main ]
  pull_request:
  schedule:
    - cron: '0 0 * * *'  # Daily at midnight

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.10'
      - run: pip install -e ".[dev]"
      - run: pytest tests/ --cov=gridcraft_mcp
  
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.10'
      - run: pip install -e ".[dev]"
      - run: black --check gridcraft_mcp
      - run: flake8 gridcraft_mcp
      - run: mypy gridcraft_mcp
```

---

## Best Practices

### Test Organization

1. **Group related tests:** Use test classes to group related tests
   ```python
   class TestFileOperations:
       def test_open_workbook(self): ...
       def test_save_workbook(self): ...
   ```

2. **Use descriptive names:** Test names should describe what they test
   ```python
   # Good
   def test_read_range_returns_correct_data(self):
   
   # Bad
   def test_read_range(self):
   ```

3. **Keep tests independent:** Each test should work in isolation

### Test Quality

1. **Test both success and failure paths:**
   ```python
   def test_success_path(self):
       # Test successful execution
   
   def test_error_path(self):
       # Test error handling
   ```

2. **Test edge cases:**
   - Empty data
   - Large data
   - Invalid parameters
   - Network errors
   - Timeout scenarios

3. **Avoid testing implementation details:** Test behavior, not internals

4. **Use meaningful assertions:**
   ```python
   # Good
   assert result == expected_value
   assert isinstance(result, type)
   
   # Better
   assert result == expected_value, f"Expected {expected_value}, got {result}"
   ```

### Performance

1. **Keep unit tests fast:** < 100ms per test
2. **Use mocks for slow operations:** Network, file I/O
3. **Test performance separately:** Create performance test suite

### Documentation

1. **Document test purpose:** Each test class should have a docstring
   ```python
   class TestFileOperations:
       """Test suite for file operation methods"""
   ```

2. **Document test cases:** Each test method should have a docstring
   ```python
   def test_open_workbook(self):
       """Test opening an existing workbook"""
   ```

3. **Update tests when features change:** Keep tests in sync with code

---

## Debugging Tests

### Common Issues

**Issue: Tests fail but should pass**
- Check mocks are correctly configured
- Verify test data is correct
- Check assertions are accurate

**Issue: Tests pass locally but fail in CI**
- Check environment differences
- Verify dependencies are installed
- Check file paths

**Issue: Integration tests fail**
- Verify GridCraft CLI is available
- Check MCP server is running
- Verify port is correct

### Debugging Tools

```python
# Enable debug logging
import logging
logging.basicConfig(level=logging.DEBUG)

# Use pdb for debugging
import pdb; pdb.set_trace()

# Print debug information
print(f"Debug info: {variable}")

# Use pytest -s for output
pytest tests/ -v -s
```

### Debugging Integration Tests

```bash
# Run with verbose output
pytest tests/test_integration.py -v -s

# Run specific test
pytest tests/test_integration.py::TestIntegration::test_create_and_read_workbook -v -s

# Check GridCraft logs
# GridCraft writes logs to:
# - Linux: ~/.config/gridcraft/logs/gridcraft.log
# - macOS: ~/Library/Application Support/GridCraft/logs/gridcraft.log
# - Windows: %APPDATA%\GridCraft\logs\gridcraft.log
```

---

## Test Maintenance

### When to Update Tests

1. **When adding new features:** Add corresponding tests
2. **When fixing bugs:** Add regression tests
3. **When refactoring:** Update tests to match new behavior
4. **When dependencies change:** Update mocks and fixtures

### Test Maintenance Checklist

- [ ] Tests pass with current code
- [ ] Tests cover all public API methods
- [ ] Tests cover error cases
- [ ] Tests are fast (< 100ms per test)
- [ ] Tests are well-documented
- [ ] Tests follow project conventions

### Deprecation

When deprecating features:
1. Mark tests as deprecated
2. Add deprecation warnings
3. Update documentation
4. Plan removal in future version

---

## Resources

- [pytest documentation](https://docs.pytest.org/)
- [unittest.mock documentation](https://docs.python.org/3/library/unittest.mock.html)
- [Test-Driven Development (TDD)](https://en.wikipedia.org/wiki/Test-driven_development)
- [Python Testing Best Practices](https://realpython.com/python-testing/)

---

## Summary

The GridCraft MCP Python client has a comprehensive test suite with:
- **60+ tests** covering all functionality
- **Unit tests** for isolated component testing
- **Integration tests** for real-world scenarios
- **Example validation** to ensure examples work
- **CI/CD integration** for continuous quality

**Test Command Summary:**
```bash
# Run all tests
pytest tests/ -v

# Run unit tests only
pytest tests/test_client.py -v

# Run integration tests
pytest tests/test_integration.py -v

# Check coverage
pytest tests/ --cov=gridcraft_mcp
```

This comprehensive test suite ensures the GridCraft MCP client is reliable, maintainable, and ready for production use.
