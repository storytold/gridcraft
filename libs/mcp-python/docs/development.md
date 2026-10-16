# Development Guide

This document provides guidelines for developing the GridCraft MCP Python client.

## Setup Development Environment

### Prerequisites

- Python 3.8+
- pip
- Git
- GridCraft with MCP server (for testing)

### Installation

```bash
# Clone the repository
git clone https://github.com/storytold/gridcraft.git
cd gridcraft/gridcraft-python

# Install in development mode
pip install -e ".[dev]"
```

### Install GridCraft CLI

For testing, you need the GridCraft CLI with MCP server support:

```bash
# From GridCraft repository root
cd gridcraft
cargo build --release -p gridcraft-cli
# The binary will be at target/release/gridcraft-cli
```

## Development Workflow

### Code Style

We use:
- **Black** for code formatting
- **Flake8** for linting
- **Mypy** for type checking
- **isort** for import sorting

Run formatting and linting:

```bash
# Format code
black gridcraft_mcp

# Sort imports
isort gridcraft_mcp

# Run linter
flake8 gridcraft_mcp

# Run type checker
mypy gridcraft_mcp
```

### Testing

Run the test suite:

```bash
# Run all tests
pytest

# Run with coverage
pytest --cov=gridcraft_mcp --cov-report=html

# Run specific test
pytest tests/test_client.py -v

# Run tests and show coverage in terminal
pytest --cov=gridcraft_mcp --cov-report=term-missing
```

### Writing Tests

Follow these patterns when writing tests:

```python
import pytest
from unittest.mock import Mock, patch


def test_feature_name():
    """Clear description of what is being tested"""
    # Setup
    client = GridCraftClient()
    
    # Mock dependencies
    with patch('module.function') as mock_function:
        mock_function.return_value = "expected"
        
        # Execute
        result = client.method()
        
        # Assert
        assert result == "expected"
        mock_function.assert_called_once()
```

### Documentation

Update documentation when:
- Adding new features
- Changing existing behavior
- Adding new parameters

Use docstrings with Google style:

```python
def method_name(param1: str, param2: int = 1) -> bool:
    """
    Clear description of what the method does.
    
    Args:
        param1: Description of first parameter
        param2: Description of second parameter (default: 1)
        
    Returns:
        Description of return value
        
    Raises:
        ExceptionType: When this exception is raised
        
    Examples:
        >>> method_name("test", 2)
        True
    """
```

## Adding New Features

### Step 1: Design

1. **Identify the use case** - What problem does this solve?
2. **Check existing API** - Is there already a way to do this?
3. **Design the interface** - How should users interact with this feature?
4. **Consider backward compatibility** - Will this break existing code?

### Step 2: Implement

1. **Add the method** to the appropriate class
2. **Write tests** for the new feature
3. **Update documentation**
4. **Add examples** if applicable

### Step 3: Test

1. **Unit tests** - Test individual components
2. **Integration tests** - Test with real GridCraft instance
3. **Manual testing** - Try different scenarios

### Step 4: Document

1. **Update README.md** with new feature
2. **Update API reference** in documentation
3. **Add examples** to `examples/` directory
4. **Update CHANGELOG.md** (if applicable)

## Release Process

### Versioning

Follow [Semantic Versioning](https://semver.org/):
- **MAJOR** version when you make incompatible API changes
- **MINOR** version when you add functionality in a backward compatible manner
- **PATCH** version when you make backward compatible bug fixes

### Steps

1. **Update version** in `pyproject.toml`
2. **Update CHANGELOG.md** with release notes
3. **Run tests** to ensure everything works
4. **Build package**
   ```bash
   python -m build
   ```
5. **Upload to PyPI**
   ```bash
   twine upload dist/*
   ```
6. **Create Git tag**
   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

## Continuous Integration

The project uses GitHub Actions for CI. Tests run automatically on:
- Push to main branch
- Pull requests
- Scheduled runs

### Local CI Testing

You can run CI checks locally:

```bash
# Format check
black --check gridcraft_mcp

# Lint check
flake8 gridcraft_mcp

# Type check
mypy gridcraft_mcp

# Test
pytest
```

## Best Practices

### Error Handling

- Use custom exceptions for GridCraft-specific errors
- Provide clear error messages
- Include context in exceptions
- Don't expose internal errors to users

### Performance

- Avoid unnecessary network calls
- Cache results when appropriate
- Use efficient data structures
- Consider async support for future versions

### Security

- Validate all user input
- Use HTTPS for network requests
- Handle authentication securely
- Don't log sensitive data

### Code Organization

- Keep methods focused and small (< 50 lines)
- Use type hints for better IDE support
- Document public APIs thoroughly
- Follow existing code patterns

## Debugging

### Common Issues

**Connection refused:**
- Ensure GridCraft is running with MCP server
- Check port number
- Verify host address

**Command failed:**
- Check command parameters
- Verify command exists in GridCraft
- Read error response

**Timeout errors:**
- Increase timeout parameter
- Check network connectivity
- Verify GridCraft is responsive

### Debugging Tools

```python
# Enable debug logging
import logging
logging.basicConfig(level=logging.DEBUG)

# Use try/except with detailed error handling
try:
    client.method()
except Exception as e:
    print(f"Error: {e}")
    import traceback
    traceback.print_exc()
```

## Contributing

1. **Fork the repository**
2. **Create a feature branch** (`git checkout -b feature/your-feature`)
3. **Commit your changes** (`git commit -am 'Add some feature'`)
4. **Push to the branch** (`git push origin feature/your-feature`)
5. **Create a Pull Request**

### Code Review Checklist

- [ ] Code follows existing patterns
- [ ] Tests are comprehensive
- [ ] Documentation is updated
- [ ] Examples are provided (if applicable)
- [ ] CI passes
- [ ] No breaking changes (unless intentional)

## Resources

- [GridCraft Documentation](https://github.com/storytold/gridcraft/tree/main/docs)
- [GridCraft MCP Server Docs](https://github.com/storytold/gridcraft/tree/main/docs/mcp.md)
- [Python MCP Client](https://github.com/modelcontextprotocol/python-sdk)
- [PyPI Packaging Guide](https://packaging.python.org/en/latest/)
