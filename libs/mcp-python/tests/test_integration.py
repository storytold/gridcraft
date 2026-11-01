#!/usr/bin/env python3
"""
Integration tests for GridCraft MCP client.

These tests require a running GridCraft instance with MCP server enabled.
They are skipped by default and can be run with:
    pytest tests/test_integration.py -v
    or
    pytest tests/test_integration.py::TestIntegration -v
"""

import pytest
import os
import subprocess
import time
from pathlib import Path
from gridcraft_mcp import GridCraftClient, ConnectionError, CommandError


# Check if GridCraft CLI is available
def is_gridcraft_available():
    """Check if gridcraft-cli is available in PATH or specified path"""
    gridcraft_path = os.getenv("GRIDCRAFT_CLI_PATH", "gridcraft-cli")
    
    # Check if file exists
    if not Path(gridcraft_path).exists():
        return False, f"GridCraft CLI not found at {gridcraft_path}"
    
    # Check if it's executable
    if not os.access(gridcraft_path, os.X_OK):
        return False, f"GridCraft CLI at {gridcraft_path} is not executable"
    
    return True, gridcraft_path


# Skip integration tests if GridCraft is not available
gridcraft_available, gridcraft_path = is_gridcraft_available()


def pytest_configure(config):
    """Configure pytest to skip integration tests if GridCraft is not available"""
    if not gridcraft_available:
        config.option.markexpr = "not integration"


@pytest.fixture(scope="module")
def gridcraft_client():
    """
    Fixture that starts GridCraft MCP server and provides a client.
    
    Yields:
        GridCraftClient: Configured client
        
    Cleanup:
        Stops the GridCraft process
    """
    if not gridcraft_available:
        pytest.skip("GridCraft CLI not available")
    
    client = GridCraftClient(port=7979)
    
    # Start GridCraft with MCP server
    try:
        client.start(executable=gridcraft_path)
        yield client
    except ConnectionError as e:
        pytest.skip(f"Failed to start GridCraft: {e}")
    finally:
        client.stop()


@pytest.fixture
def clean_workbook(gridcraft_client):
    """
    Fixture that creates a clean workbook for each test.
    
    Yields:
        GridCraftClient: Client with new workbook
    """
    client = gridcraft_client
    
    # Create new workbook
    client.new_workbook()
    
    yield client
    
    # Clean up - workbook is discarded after each test


class TestIntegration:
    """Integration tests for GridCraft MCP client"""
    
    def test_client_connection(self, gridcraft_client):
        """Test that client can connect to GridCraft MCP server"""
        assert gridcraft_client.process is not None
        assert gridcraft_client.process.poll() is None
    
    def test_create_and_read_workbook(self, clean_workbook):
        """Test creating a workbook and reading data"""
        client = clean_workbook
        
        # Write some data
        client.set_cell("A1", "Product")
        client.set_cell("B1", "Sales")
        client.set_cell("A2", "Widget A")
        client.set_cell("B2", 100)
        client.set_cell("A3", "Widget B")
        client.set_cell("B3", 150)
        
        # Read data back
        data = client.read_range("A1:B3")
        
        assert len(data) == 3
        assert data[0] == ["Product", "Sales"]
        assert data[1] == ["Widget A", 100]
        assert data[2] == ["Widget B", 150]
    
    def test_formula_evaluation(self, clean_workbook):
        """Test evaluating formulas"""
        client = clean_workbook
        
        # Set up data
        client.set_cell("A1", 10)
        client.set_cell("A2", 20)
        client.set_cell("A3", 30)
        
        # Test SUM
        total = client.evaluate_formula("=SUM(A1:A3)")
        assert total == 60
        
        # Test AVERAGE
        avg = client.evaluate_formula("=AVERAGE(A1:A3)")
        assert avg == 20
        
        # Test MAX
        max_val = client.evaluate_formula("=MAX(A1:A3)")
        assert max_val == 30
        
        # Test MIN
        min_val = client.evaluate_formula("=MIN(A1:A3)")
        assert min_val == 10
    
    def test_write_and_read_range(self, clean_workbook):
        """Test writing and reading ranges"""
        client = clean_workbook
        
        # Prepare data
        data = [
            ["Date", "Product", "Sales"],
            ["2024-01-01", "Laptop", 150],
            ["2024-01-02", "Phone", 200],
            ["2024-01-03", "Tablet", 120],
        ]
        
        # Write range
        result = client.write_range("A1:C4", data)
        assert result is True
        
        # Read range back
        read_data = client.read_range("A1:C4")
        
        assert read_data == data
    
    def test_create_table(self, clean_workbook):
        """Test creating structured tables"""
        client = clean_workbook
        
        # Create table
        table_info = client.create_table(
            range_str="A1:D10",
            name="TestTable",
            has_headers=True
        )
        
        assert "id" in table_info
        assert "name" in table_info
        assert table_info["name"] == "TestTable"
    
    def test_insert_chart(self, clean_workbook):
        """Test inserting charts"""
        client = clean_workbook
        
        # Set up data
        client.set_cell("A1", "Product")
        client.set_cell("B1", "Sales")
        client.set_cell("A2", "Laptop")
        client.set_cell("B2", 150)
        client.set_cell("A3", "Phone")
        client.set_cell("B3", 200)
        
        # Insert column chart
        chart_info = client.insert_chart(
            range_str="A1:B3",
            chart_type="column",
            title="Sales Chart"
        )
        
        assert "id" in chart_info
        assert chart_info["type"] == "column"
    
    def test_sort_range(self, clean_workbook):
        """Test sorting ranges"""
        client = clean_workbook
        
        # Set up unsorted data
        client.set_cell("A1", "Product")
        client.set_cell("B1", "Sales")
        client.set_cell("A2", "C")
        client.set_cell("B2", 300)
        client.set_cell("A3", "A")
        client.set_cell("B3", 100)
        client.set_cell("A4", "B")
        client.set_cell("B4", 200)
        
        # Sort by sales (descending)
        result = client.sort_range(
            range_str="A1:B4",
            sort_by=[{"column": 1, "order": "descending"}]
        )
        assert result is True
        
        # Read back and verify sort order
        data = client.read_range("A1:B4")
        
        # Check that data is sorted by sales (column B, index 1)
        sales_values = [row[1] for row in data[1:]]
        assert sales_values == sorted(sales_values, reverse=True)
    
    def test_file_operations(self, gridcraft_client):
        """Test file operations"""
        client = gridcraft_client
        
        # Create a temporary file
        test_file = "test_integration.xlsx"
        
        # Create new workbook
        client.new_workbook()
        client.set_cell("A1", "Test Data")
        
        # Save workbook
        result = client.save_workbook(test_file)
        assert result is True
        
        # Verify file exists
        assert Path(test_file).exists()
        
        # Open the file again
        result = client.open_workbook(test_file)
        assert result is True
        
        # Verify data is still there
        data = client.read_range("A1:A1")
        assert data[0][0] == "Test Data"
        
        # Clean up
        if Path(test_file).exists():
            Path(test_file).unlink()
    
    def test_sheet_operations(self, clean_workbook):
        """Test sheet operations"""
        client = clean_workbook
        
        # Add new sheet
        result = client.add_sheet("TestSheet")
        assert result is True
        
        # List sheets
        sheets = client.list_sheets()
        assert "TestSheet" in sheets
        assert "Sheet1" in sheets
        
        # Rename sheet
        result = client.rename_sheet("TestSheet", "RenamedSheet")
        assert result is True
        
        sheets = client.list_sheets()
        assert "RenamedSheet" in sheets
        assert "TestSheet" not in sheets
        
        # Delete sheet
        result = client.delete_sheet("RenamedSheet")
        assert result is True
        
        sheets = client.list_sheets()
        assert "RenamedSheet" not in sheets
    
    def test_conditional_formatting(self, clean_workbook):
        """Test applying conditional formatting"""
        client = clean_workbook
        
        # Set up data
        client.set_cell("A1", "Value")
        client.set_cell("A2", 10)
        client.set_cell("A3", 20)
        client.set_cell("A4", 30)
        client.set_cell("A5", 5)
        
        # Apply data bars
        result = client.format_range("A2:A5", "data_bar")
        assert result is True
        
        # Apply conditional formatting (top 2 values)
        result = client.format_range("A2:A5", "conditional", {
            "type": "top_n",
            "value": 2,
            "format": "bold",
            "color": "green"
        })
        assert result is True
    
    def test_export_csv(self, clean_workbook):
        """Test exporting to CSV"""
        client = clean_workbook
        
        # Set up data
        client.set_cell("A1", "Product")
        client.set_cell("B1", "Sales")
        client.set_cell("A2", "Laptop")
        client.set_cell("B2", 150)
        client.set_cell("A3", "Phone")
        client.set_cell("B3", 200)
        
        # Export to CSV
        csv_file = "test_export.csv"
        result = client.export_csv("A1:B3", csv_file)
        assert result is True
        
        # Verify file exists
        assert Path(csv_file).exists()
        
        # Read CSV content
        with open(csv_file, 'r') as f:
            content = f.read()
            assert "Product,Sales" in content
            assert "Laptop,150" in content
            assert "Phone,200" in content
        
        # Clean up
        if Path(csv_file).exists():
            Path(csv_file).unlink()
    
    def test_clipboard_operations(self, clean_workbook):
        """Test clipboard operations"""
        client = clean_workbook
        
        # Set up data
        client.set_cell("A1", "Copy")
        client.set_cell("A2", "Test")
        
        # Copy to clipboard
        result = client.clipboard_copy("A1:A2")
        assert result is True
        
        # Paste from clipboard
        result = client.clipboard_paste("B1:B2")
        assert result is True
        
        # Verify data was pasted
        data = client.read_range("B1:B2")
        assert data[0][0] == "Copy"
        assert data[1][0] == "Test"
    
    def test_undo_redo(self, clean_workbook):
        """Test undo and redo operations"""
        client = clean_workbook
        
        # Make a change
        client.set_cell("A1", "Original")
        
        # Change it
        client.set_cell("A1", "Modified")
        
        # Undo
        result = client.undo()
        assert result is True
        
        # Verify reverted
        value = client.evaluate_formula("=A1")
        assert value == "Original"
        
        # Redo
        result = client.redo()
        assert result is True
        
        # Verify redone
        value = client.evaluate_formula("=A1")
        assert value == "Modified"
    
    def test_working_with_multiple_sheets(self, clean_workbook):
        """Test working with multiple sheets"""
        client = clean_workbook
        
        # Add second sheet
        client.add_sheet("DataSheet")
        
        # Write to different sheets
        client.set_cell("A1", "Sheet1 Data", sheet="Sheet1")
        client.set_cell("A1", "Sheet2 Data", sheet="DataSheet")
        
        # Read from specific sheet
        value1 = client.evaluate_formula("=A1", sheet="Sheet1")
        value2 = client.evaluate_formula("=A1", sheet="DataSheet")
        
        assert value1 == "Sheet1 Data"
        assert value2 == "Sheet2 Data"
    
    def test_error_handling(self, gridcraft_client):
        """Test error handling for invalid operations"""
        client = gridcraft_client
        
        # Try to read from non-existent range
        try:
            client.read_range("Z99:AA100")
            # If no error, that's fine - GridCraft might return empty data
        except CommandError:
            # Expected - range doesn't exist
            pass
        
        # Try to evaluate invalid formula
        try:
            client.evaluate_formula("=INVALID_FORMULA()")
            # If no error, that's fine
        except CommandError:
            # Expected - invalid formula
            pass


class TestPerformance:
    """Performance tests for GridCraft MCP client"""
    
    def test_large_data_operations(self, clean_workbook):
        """Test operations with larger datasets"""
        client = clean_workbook
        
        # Create a 100x10 grid of data
        data = [[f"Cell_{i}_{j}" for j in range(10)] for i in range(100)]
        
        # Write data
        start_time = time.time()
        result = client.write_range("A1:J100", data)
        write_time = time.time() - start_time
        
        assert result is True
        
        # Read data back
        start_time = time.time()
        read_data = client.read_range("A1:J100")
        read_time = time.time() - start_time
        
        assert len(read_data) == 100
        assert len(read_data[0]) == 10
        
        print(f"\nPerformance: Write 100x10 grid: {write_time:.3f}s, Read: {read_time:.3f}s")
    
    def test_multiple_operations(self, clean_workbook):
        """Test multiple sequential operations"""
        client = clean_workbook
        
        start_time = time.time()
        
        # Perform multiple operations
        for i in range(50):
            client.set_cell(f"A{i+1}", i * 10)
        
        for i in range(50):
            value = client.evaluate_formula(f"=A{i+1}")
            assert value == i * 10
        
        operation_time = time.time() - start_time
        
        print(f"\nPerformance: 100 operations in {operation_time:.3f}s")


@pytest.mark.skipif(not gridcraft_available, reason="GridCraft CLI not available")
class TestRealWorldScenarios:
    """Real-world usage scenarios"""
    
    def test_basic_spreadsheet_workflow(self, gridcraft_client):
        """Test a complete spreadsheet workflow"""
        client = gridcraft_client
        
        # Create a simple budget spreadsheet
        client.new_workbook()
        
        # Headers
        client.set_cell("A1", "Category")
        client.set_cell("B1", "Budget")
        client.set_cell("C1", "Actual")
        client.set_cell("D1", "Variance")
        
        # Data
        client.set_cell("A2", "Rent")
        client.set_cell("B2", 1000)
        client.set_cell("C2", 1000)
        
        client.set_cell("A3", "Groceries")
        client.set_cell("B3", 300)
        client.set_cell("C3", 285)
        
        client.set_cell("A4", "Utilities")
        client.set_cell("B4", 150)
        client.set_cell("C4", 162)
        
        # Formulas
        client.set_cell("D2", "=B2-C2")
        client.set_cell("D3", "=B3-C3")
        client.set_cell("D4", "=B4-C4")
        client.set_cell("B5", "=SUM(B2:B4)")
        client.set_cell("C5", "=SUM(C2:C4)")
        client.set_cell("D5", "=SUM(D2:D4)")
        
        # Formatting
        client.format_range("B2:D5", "currency")
        client.format_range("A1:D1", "bold")
        
        # Verify calculations
        total_budget = client.evaluate_formula("=B5")
        total_actual = client.evaluate_formula("=C5")
        total_variance = client.evaluate_formula("=D5")
        
        assert total_budget == 1450
        assert total_actual == 1447
        assert total_variance == 3
    
    def test_data_analysis_workflow(self, gridcraft_client):
        """Test a data analysis workflow"""
        client = gridcraft_client
        
        # Create sample sales data
        sales_data = [
            ["Date", "Product", "Region", "Sales", "Profit"],
            ["2024-01-01", "Laptop", "North", 150, 45],
            ["2024-01-02", "Phone", "South", 200, 60],
            ["2024-01-03", "Tablet", "East", 120, 36],
            ["2024-01-04", "Monitor", "West", 80, 24],
            ["2024-01-05", "Keyboard", "North", 50, 15],
            ["2024-01-06", "Mouse", "South", 30, 9],
        ]
        
        # Write data
        client.write_range("A1:E7", sales_data)
        
        # Create table
        client.create_table(range_str="A1:E7", name="SalesData")
        
        # Calculate statistics
        total_sales = client.evaluate_formula("=SUM(SalesData[Sales])")
        total_profit = client.evaluate_formula("=SUM(SalesData[Profit])")
        avg_profit = client.evaluate_formula("=AVERAGE(SalesData[Profit])")
        max_sales = client.evaluate_formula("=MAX(SalesData[Sales])")
        
        # Create chart
        client.insert_chart(
            range_str="A1:E7",
            chart_type="column",
            title="Sales by Product"
        )
        
        # Verify calculations
        assert total_sales == 630
        assert total_profit == 189
        assert avg_profit == 31.5
        assert max_sales == 200


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
