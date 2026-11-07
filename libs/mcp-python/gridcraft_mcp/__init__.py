"""
GridCraft MCP Client - Python wrapper for GridCraft's MCP server

This package provides a convenient Python interface to interact with GridCraft
spreadsheet application through its Model Context Protocol (MCP) server.
"""

from .client import GridCraftClient
from .exceptions import GridCraftError, ConnectionError, CommandError

__version__ = "0.1.0"
__all__ = ["GridCraftClient", "GridCraftError", "ConnectionError", "CommandError"]
