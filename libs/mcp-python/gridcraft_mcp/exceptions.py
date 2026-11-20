"""
Custom exceptions for GridCraft MCP client.
"""


class GridCraftError(Exception):
    """Base exception for all GridCraft errors"""

    pass


class ConnectionError(GridCraftError):
    """Raised when connection to MCP server fails"""

    pass


class CommandError(GridCraftError):
    """
    Raised when a command execution fails.

    Attributes:
        message: Error message
        command: Command that failed
        response: Full response from server
    """

    def __init__(self, message: str, command: str, response: dict = None):
        super().__init__(message)
        self.command = command
        self.response = response or {}

    def __str__(self):
        base = super().__str__()
        if self.response:
            return f"{base}\nResponse: {self.response}"
        return base


class RangeError(GridCraftError):
    """Raised when range specification is invalid"""

    pass


class SheetError(GridCraftError):
    """Raised when sheet operations fail"""

    pass


class FileError(GridCraftError):
    """Raised when file operations fail"""

    pass


class FormulaError(GridCraftError):
    """Raised when formula evaluation fails"""

    pass
