# Usage Examples

This document provides practical examples of using the GridCraft MCP Python client in various scenarios.

## Table of Contents

- [Basic Operations](#basic-operations)
- [Data Analysis](#data-analysis)
- [Automation](#automation)
- [Charts and Visualization](#charts-and-visualization)
- [Advanced Features](#advanced-features)
- [Integration with Other Tools](#integration-with-other-tools)

---

## Basic Operations

### Creating a New Workbook

```python
from gridcraft_mcp import GridCraftClient

# Auto-start GridCraft with MCP server
with GridCraftClient() as client:
    # Create new workbook
    client.new_workbook()
    print("✅ New workbook created")
```

### Writing and Reading Data

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Write data to cells
    client.set_cell("A1", "Product")
    client.set_cell("B1", "Sales")
    client.set_cell("A2", "Widget A")
    client.set_cell("B2", 100)
    client.set_cell("A3", "Widget B")
    client.set_cell("B3", 150)
    
    # Read data back
    data = client.read_range("A1:B3")
    print("Sheet content:")
    for row in data:
        print(row)
```

### Using Formulas

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Write data
    client.set_cell("A1", "Sales")
    client.set_cell("B1", "Profit")
    client.set_cell("A2", 100)
    client.set_cell("B2", 30)
    client.set_cell("A3", 150)
    client.set_cell("B3", 45)
    client.set_cell("A4", 200)
    client.set_cell("B4", 60)
    
    # Calculate totals using formulas
    total_sales = client.evaluate_formula("=SUM(A2:A4)")
    total_profit = client.evaluate_formula("=SUM(B2:B4)")
    avg_profit = client.evaluate_formula("=AVERAGE(B2:B4)")
    
    print(f"Total Sales: ${total_sales}")
    print(f"Total Profit: ${total_profit}")
    print(f"Average Profit: ${avg_profit:.2f}")
```

### Working with Multiple Sheets

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Create new workbook
    client.new_workbook()
    
    # Add sheets
    client.add_sheet("Sales")
    client.add_sheet("Expenses")
    client.add_sheet("Profit")
    
    # Write to specific sheets
    client.set_cell("A1", "Q1 Sales", sheet="Sales")
    client.set_cell("A1", "Q1 Expenses", sheet="Expenses")
    client.set_cell("A1", "Q1 Profit", sheet="Profit")
    
    # List all sheets
    sheets = client.list_sheets()
    print(f"Sheets: {sheets}")
```

---

## Data Analysis

### Creating a Data Table

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Create sample data
    data = [
        ["Date", "Product", "Region", "Sales", "Profit"],
        ["2024-01-01", "Laptop", "North", 150, 45],
        ["2024-01-02", "Phone", "South", 200, 60],
        ["2024-01-03", "Tablet", "East", 120, 36],
        ["2024-01-04", "Monitor", "West", 80, 24],
        ["2024-01-05", "Keyboard", "North", 50, 15],
    ]
    
    # Write data
    for i, row in enumerate(data):
        client.write_range(f"A{i+1}:E{i+1}", [row])
    
    # Create structured table
    table_info = client.create_table(
        range_str="A1:E6",
        name="SalesData",
        has_headers=True
    )
    print(f"Table created: {table_info}")
```

### Sorting Data

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Sort by sales (descending)
    client.sort_range(
        range_str="A1:E6",
        sort_by=[{"column": 3, "order": "descending"}]
    )
    print("✅ Data sorted by sales")
    
    # Sort by product name (ascending)
    client.sort_range(
        range_str="A1:E6",
        sort_by=[{"column": 1, "order": "ascending"}]
    )
    print("✅ Data sorted by product name")
```

### Filtering Data

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Filter for sales > 100
    client.filter_range(
        range_str="A1:E6",
        criteria={
            "column": 3,
            "operator": "greater_than",
            "value": 100
        }
    )
    print("✅ Data filtered for sales > 100")
```

### Conditional Formatting

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Highlight top 3 sales
    client.format_range("D2:D6", "conditional", {
        "type": "top_n",
        "value": 3,
        "format": "bold",
        "color": "green"
    })
    
    # Highlight negative profit (losses)
    client.format_range("E2:E6", "conditional", {
        "type": "cell_value",
        "operator": "less_than",
        "value": 0,
        "format": "red_bold"
    })
    
    # Data bars for sales
    client.format_range("D2:D6", "data_bar")
    
    print("✅ Conditional formatting applied")
```

---

## Automation

### Batch Processing Multiple Files

```python
from gridcraft_mcp import GridCraftClient
import os

# List of files to process
files = ["report1.xlsx", "report2.xlsx", "report3.xlsx"]

with GridCraftClient() as client:
    for file in files:
        if os.path.exists(file):
            print(f"Processing {file}...")
            
            # Open file
            client.open_workbook(file)
            
            # Update formulas
            client.execute_command("calc.recalculate")
            
            # Save with new name
            new_name = f"updated_{file}"
            client.save_workbook(new_name)
            
            print(f"✅ Processed {file} -> {new_name}")
```

### Generating Reports from Templates

```python
from gridcraft_mcp import GridCraftClient
from datetime import datetime

with GridCraftClient() as client:
    # Open template
    client.open_workbook("monthly_template.xlsx")
    
    # Fill in dynamic data
    client.set_cell("B1", f"Monthly Report - {datetime.now().strftime('%B %Y')}")
    client.set_cell("B2", datetime.now().strftime("%Y-%m-%d"))
    
    # Update calculations
    client.execute_command("calc.recalculate")
    
    # Save as new report
    report_name = f"monthly_report_{datetime.now().strftime('%Y%m%d')}.xlsx"
    client.save_workbook(report_name)
    
    print(f"✅ Report generated: {report_name}")
```

### Scheduled Data Updates

```python
from gridcraft_mcp import GridCraftClient
import schedule
import time

def update_sales_data():
    """Function to update sales data"""
    with GridCraftClient() as client:
        # Fetch latest data (in real scenario, from API/database)
        latest_sales = fetch_latest_sales()  # Your data fetching function
        
        # Update spreadsheet
        for i, sale in enumerate(latest_sales, start=2):
            client.set_cell(f"A{i}", sale['date'])
            client.set_cell(f"B{i}", sale['product'])
            client.set_cell(f"C{i}", sale['amount'])
        
        # Recalculate
        client.execute_command("calc.recalculate")
        
        print("✅ Sales data updated")

# Schedule updates
schedule.every().day.at("09:00").do(update_sales_data)

print("🤖 Sales data updater started. Press Ctrl+C to stop.")

# Run scheduler
while True:
    schedule.run_pending()
    time.sleep(60)
```

---

## Charts and Visualization

### Column Chart

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Sample data
    data = [
        ["Product", "Sales"],
        ["Laptop", 150],
        ["Phone", 200],
        ["Tablet", 120],
        ["Monitor", 80],
    ]
    
    # Write data
    for i, row in enumerate(data):
        client.write_range(f"A{i+1}:B{i+1}", [row])
    
    # Create column chart
    chart_info = client.insert_chart(
        range_str="A1:B5",
        chart_type="column",
        title="Sales by Product",
        x_axis="Product",
        y_axis="Sales ($)"
    )
    
    print(f"✅ Column chart created with ID: {chart_info.get('id')}")
```

### Line Chart (Trend Analysis)

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Quarterly data
    quarters = ["Q1", "Q2", "Q3", "Q4"]
    revenue = [100000, 120000, 110000, 130000]
    
    # Write data
    for i, (q, r) in enumerate(zip(quarters, revenue), start=2):
        client.set_cell(f"A{i}", q)
        client.set_cell(f"B{i}", r)
    
    # Create line chart
    chart_info = client.insert_chart(
        range_str="A1:B5",
        chart_type="line",
        title="Quarterly Revenue Trend",
        x_axis="Quarter",
        y_axis="Revenue ($)"
    )
    
    print(f"✅ Line chart created with ID: {chart_info.get('id')}")
```

### Pie Chart (Distribution)

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Market share data
    products = ["Windows", "Mac", "Linux", "ChromeOS"]
    market_share = [72.5, 14.8, 2.6, 3.8]
    
    # Write data
    for i, (product, share) in enumerate(zip(products, market_share), start=2):
        client.set_cell(f"A{i}", product)
        client.set_cell(f"B{i}", share)
    
    # Create pie chart
    chart_info = client.insert_chart(
        range_str="A1:B6",
        chart_type="pie",
        title="Market Share by OS",
        x_axis="Operating System",
        y_axis="Market Share (%)"
    )
    
    print(f"✅ Pie chart created with ID: {chart_info.get('id')}")
```

### Multiple Charts in Dashboard

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Create dashboard layout
    client.set_cell("A1", "Sales Dashboard", sheet="Sheet1")
    client.format_range("A1:D1", "bold")
    client.format_range("A1:D1", "font_size", 16)
    
    # Add summary metrics
    client.set_cell("A3", "Total Sales")
    client.set_cell("B3", "=SUM(B2:B100)")
    client.format_range("B3", "currency")
    
    client.set_cell("A4", "Average Sale")
    client.set_cell("B4", "=AVERAGE(B2:B100)")
    client.format_range("B4", "currency")
    
    # Create charts
    chart1 = client.insert_chart(
        range_str="A1:B100",
        chart_type="column",
        title="Sales by Product"
    )
    
    chart2 = client.insert_chart(
        range_str="A1:B100",
        chart_type="line",
        title="Sales Trend"
    )
    
    chart3 = client.insert_chart(
        range_str="A1:B100",
        chart_type="pie",
        title="Sales Distribution"
    )
    
    print(f"✅ Dashboard created with {chart1.get('id')}, {chart2.get('id')}, {chart3.get('id')}")
```

---

## Advanced Features

### Working with Tables

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Create table
    table_info = client.create_table(
        range_str="A1:D10",
        name="SalesTable",
        has_headers=True
    )
    
    # Add data to table
    client.write_range("A2:D2", [["2024-01-01", "Product A", "North", 100]])
    client.write_range("A3:D3", [["2024-01-02", "Product B", "South", 150]])
    
    # Use structured references
    total = client.evaluate_formula("=SUM(SalesTable[Sales])")
    print(f"Total sales: ${total}")
    
    # Table operations
    client.execute_command("table.sort", name="SalesTable", column="Sales", descending=True)
    
    print(f"✅ Table operations completed")
```

### Exporting Data

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Export to CSV
    client.export_csv("A1:D100", "sales_data.csv")
    print("✅ Data exported to sales_data.csv")
    
    # Export to PDF (if supported)
    try:
        client.execute_command("file.export_pdf", path="report.pdf")
        print("✅ Report exported to report.pdf")
    except Exception as e:
        print(f"⚠️ PDF export not available: {e}")
```

### Clipboard Operations

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Copy range to clipboard
    client.clipboard_copy("A1:B10")
    print("✅ Range copied to clipboard")
    
    # Paste from clipboard to new location
    client.clipboard_paste("C1:C10")
    print("✅ Data pasted from clipboard")
```

### Undo/Redo Operations

```python
from gridcraft_mcp import GridCraftClient

with GridCraftClient() as client:
    # Make some changes
    client.set_cell("A1", "Test")
    
    # Undo last action
    client.undo()
    print("✅ Last action undone")
    
    # Redo
    client.redo()
    print("✅ Action redone")
```

---

## Integration with Other Tools

### Integration with Pandas

```python
from gridcraft_mcp import GridCraftClient
import pandas as pd

# Sample DataFrame
df = pd.DataFrame({
    'Product': ['Laptop', 'Phone', 'Tablet'],
    'Sales': [150, 200, 120],
    'Profit': [45, 60, 36]
})

with GridCraftClient() as client:
    # Write DataFrame to GridCraft
    client.write_range("A1:C4", [df.columns.tolist()] + df.values.tolist())
    
    # Read back as DataFrame
    data = client.read_range("A1:C4")
    headers = data[0]
    rows = data[1:]
    
    df_read = pd.DataFrame(rows, columns=headers)
    print("DataFrame from GridCraft:")
    print(df_read)
```

### Integration with Matplotlib

```python
from gridcraft_mcp import GridCraftClient
import matplotlib.pyplot as plt

with GridCraftClient() as client:
    # Get data from GridCraft
    data = client.read_range("A1:B5")
    
    # Extract data
    products = [row[0] for row in data[1:]]
    sales = [int(row[1]) for row in data[1:]]
    
    # Create matplotlib chart
    plt.figure(figsize=(10, 6))
    plt.bar(products, sales)
    plt.title("Sales by Product")
    plt.xlabel("Product")
    plt.ylabel("Sales")
    plt.xticks(rotation=45)
    plt.tight_layout()
    
    # Save chart
    plt.savefig("matplotlib_chart.png")
    print("✅ Matplotlib chart saved as matplotlib_chart.png")
    
    # Optionally insert into GridCraft
    # (This would require additional functionality)
```

### Integration with NumPy

```python
from gridcraft_mcp import GridCraftClient
import numpy as np

with GridCraftClient() as client:
    # Generate random data
    data = np.random.rand(10, 3) * 100
    
    # Write to GridCraft
    client.write_range("A1:C11", data.tolist())
    
    # Calculate statistics using GridCraft formulas
    mean_val = client.evaluate_formula("=AVERAGE(A1:C11)")
    std_val = client.evaluate_formula("=STDEV(A1:C11)")
    
    print(f"Mean: {mean_val:.2f}")
    print(f"Standard Deviation: {std_val:.2f}")
```

---

## Best Practices

### Error Handling

```python
from gridcraft_mcp import GridCraftClient, ConnectionError, CommandError

try:
    with GridCraftClient() as client:
        # Your operations here
        client.new_workbook()
        client.set_cell("A1", "Hello")
        
except ConnectionError as e:
    print(f"❌ Failed to connect: {e}")
    # Handle connection issues
    
except CommandError as e:
    print(f"❌ Command failed: {e}")
    print(f"   Command: {e.command}")
    if e.response:
        print(f"   Response: {e.response}")
    # Handle command-specific errors
    
except Exception as e:
    print(f"❌ Unexpected error: {e}")
    import traceback
    traceback.print_exc()
    # Handle unexpected errors
```

### Logging

```python
from gridcraft_mcp import GridCraftClient
import logging

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)

with GridCraftClient() as client:
    try:
        logger.info("Starting operations")
        
        client.new_workbook()
        logger.info("Created new workbook")
        
        client.set_cell("A1", "Test")
        logger.info("Set cell A1")
        
        data = client.read_range("A1:A1")
        logger.info(f"Read data: {data}")
        
    except Exception as e:
        logger.error(f"Operation failed: {e}")
        raise
```

### Configuration Management

```python
from gridcraft_mcp import GridCraftClient
import os
from dataclasses import dataclass

@dataclass
class Config:
    gridcraft_path: str = "gridcraft-cli"
    port: int = 7979
    timeout: int = 60
    log_level: str = "INFO"

def get_config():
    """Load configuration from environment or use defaults"""
    return Config(
        gridcraft_path=os.getenv("GRIDCRAFT_CLI_PATH", "gridcraft-cli"),
        port=int(os.getenv("GRIDCRAFT_PORT", "7979")),
        timeout=int(os.getenv("GRIDCRAFT_TIMEOUT", "60")),
        log_level=os.getenv("GRIDCRAFT_LOG_LEVEL", "INFO")
    )

config = get_config()

with GridCraftClient(port=config.port, timeout=config.timeout) as client:
    # Use client
    pass
```

---

## Troubleshooting

### Common Issues and Solutions

**Issue: Connection refused**
- **Cause**: GridCraft MCP server not running
- **Solution**: Start GridCraft with `gridcraft-cli mcp --connect 7979`

**Issue: Command failed with no details**
- **Cause**: GridCraft doesn't support the command
- **Solution**: Check GridCraft documentation for available commands

**Issue: Timeout errors**
- **Cause**: Slow operations or network issues
- **Solution**: Increase timeout parameter or optimize operations

**Issue: Data format issues**
- **Cause**: Type mismatches between Python and GridCraft
- **Solution**: Convert data to appropriate types before sending

### Debugging Tips

1. **Enable logging**: Set `RUST_LOG=debug` when starting GridCraft
2. **Check server logs**: Look at GridCraft's log files
3. **Test with CLI first**: Verify commands work with `gridcraft-cli` before using Python
4. **Use try/except**: Wrap operations in error handling
5. **Validate data**: Check data formats before sending to GridCraft

---

## More Examples

See the `examples/` directory in the repository for:
- `basic_usage.py` - Basic operations
- `data_analysis.py` - Data analysis workflow
- `automation.py` - Automation examples
- `charts_and_tables.py` - Advanced charting

Each example demonstrates real-world use cases and can be adapted for your specific needs.
