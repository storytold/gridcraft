"""
Pytest configuration for GridCraft MCP client tests.
"""

import pytest


def pytest_addoption(parser):
    """Add custom command-line options"""
    parser.addoption(
        "--integration",
        action="store_true",
        default=False,
        help="Run integration tests (require GridCraft CLI)"
    )
    
    parser.addoption(
        "--gridcraft-path",
        default="gridcraft-cli",
        help="Path to gridcraft-cli executable"
    )


def pytest_configure(config):
    """Configure pytest based on command-line options"""
    if not config.getoption("--integration"):
        # Skip integration tests by default
        setattr(config.option, 'markexpr', 'not integration')


@pytest.fixture
def sample_data():
    """Fixture providing sample data for testing"""
    return {
        "headers": ["Product", "Sales", "Profit"],
        "rows": [
            ["Laptop", 150, 45],
            ["Phone", 200, 60],
            ["Tablet", 120, 36],
        ]
    }


@pytest.fixture
def empty_client():
    """Fixture providing an empty GridCraftClient instance"""
    from gridcraft_mcp import GridCraftClient
    return GridCraftClient()
