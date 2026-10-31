#!/usr/bin/env python3
"""
Automation example using GridCraft MCP client.

This example demonstrates how to automate repetitive tasks with GridCraft.
"""

import os
from gridcraft_mcp import GridCraftClient


def create_monthly_report(client, month, year, sales_data):
    """
    Create a monthly sales report

    Args:
        client: GridCraftClient instance
        month: Month name
        year: Year
        sales_data: List of sales records
    """
    print(f"\n📅 Creating {month} {year} report...")

    # Create new workbook
    client.new_workbook()

    # Set up headers
    client.set_cell("A1", "Date")
    client.set_cell("B1", "Product")
    client.set_cell("C1", "Region")
    client.set_cell("D1", "Sales")
    client.set_cell("E1", "Profit")
    client.set_cell("F1", "Margin %")

    # Write data
    for i, record in enumerate(sales_data, start=2):
        date, product, region, sales, profit = record
        margin = (profit / sales) * 100 if sales > 0 else 0

        client.set_cell(f"A{i}", date)
        client.set_cell(f"B{i}", product)
        client.set_cell(f"C{i}", region)
        client.set_cell(f"D{i}", sales)
        client.set_cell(f"E{i}", profit)
        client.set_cell(f"F{i}", f"{margin:.1f}%")

    # Create table
    client.create_table(range_str="A1:F101", name=f"{month}_{year}_Sales")

    # Calculate totals
    total_sales = client.evaluate_formula("=SUM(D2:D101)")
    total_profit = client.evaluate_formula("=SUM(E2:E101)")
    avg_margin = client.evaluate_formula("=AVERAGE(F2:F101)")

    # Create summary section
    client.set_cell("H1", "Report Summary")
    client.set_cell("H2", "Period")
    client.set_cell("H3", "Total Sales")
    client.set_cell("H4", "Total Profit")
    client.set_cell("H5", "Average Margin")

    client.set_cell("I2", f"{month} {year}")
    client.set_cell("I3", f"${total_sales:,.2f}")
    client.set_cell("I4", f"${total_profit:,.2f}")
    client.set_cell("I5", f"{avg_margin:.2f}%")

    # Format summary
    client.format_range("H1:I1", "bold")
    client.format_range("H2:I5", "")
    client.format_range("I3:I5", "currency")

    # Create charts
    client.insert_chart(
        range_str="A1:F101",
        chart_type="column",
        title=f"Sales by Product - {month} {year}",
    )

    client.insert_chart(
        range_str="A1:F101",
        chart_type="pie",
        title=f"Profit by Region - {month} {year}",
    )

    # Save report
    filename = f"{month.lower()}_{year}_sales_report.xlsx"
    client.save_workbook(filename)

    print(f"✅ Report created: {filename}")
    return filename


def process_multiple_months():
    """Process data for multiple months"""

    # Sample data for 3 months
    data = {
        "January": [
            ["2024-01-01", "Laptop", "North", 150, 45],
            ["2024-01-02", "Phone", "South", 200, 60],
            ["2024-01-03", "Tablet", "East", 120, 36],
        ],
        "February": [
            ["2024-02-01", "Laptop", "North", 180, 54],
            ["2024-02-02", "Phone", "South", 190, 57],
            ["2024-02-03", "Tablet", "East", 130, 39],
        ],
        "March": [
            ["2024-03-01", "Laptop", "North", 200, 60],
            ["2024-03-02", "Phone", "South", 210, 63],
            ["2024-03-03", "Tablet", "East", 140, 42],
        ],
    }

    reports = []

    with GridCraftClient() as client:
        print("🤖 Starting automated report generation...")
        print("=" * 60)

        for month, sales_data in data.items():
            report = create_monthly_report(client, month, 2024, sales_data)
            reports.append(report)

        # Create quarterly summary
        print("\n📊 Creating quarterly summary...")
        client.new_workbook()

        client.set_cell("A1", "Month")
        client.set_cell("B1", "Total Sales")
        client.set_cell("C1", "Total Profit")

        for i, (month, _) in enumerate(data.items(), start=2):
            # In real scenario, we would read from each report
            # For this example, we'll simulate
            sales = (i - 1) * 1000
            profit = sales * 0.3

            client.set_cell(f"A{i}", month)
            client.set_cell(f"B{i}", sales)
            client.set_cell(f"C{i}", profit)

        # Calculate quarterly totals
        total_sales = client.evaluate_formula("=SUM(B2:B4)")
        total_profit = client.evaluate_formula("=SUM(C2:C4)")

        client.set_cell("A5", "Quarter Total")
        client.set_cell("B5", total_sales)
        client.set_cell("C5", total_profit)

        # Format
        client.format_range("B2:C5", "currency")
        client.format_range("A5:C5", "bold")

        quarterly_report = "Q1_2024_Summary.xlsx"
        client.save_workbook(quarterly_report)
        reports.append(quarterly_report)

        print(f"✅ Quarterly summary created: {quarterly_report}")

        # Clean up old reports (optional)
        print("\n🧹 Cleaning up...")
        for report in reports:
            if os.path.exists(report):
                print(f"  Removed: {report}")

        print("\n✅ Automation workflow completed successfully!")
        print(f"📁 Generated {len(reports)} reports")

    return 0


def batch_update_workbooks():
    """Update multiple workbooks with new data"""

    workbooks = [
        "sales_report_jan.xlsx",
        "sales_report_feb.xlsx",
        "sales_report_mar.xlsx",
    ]

    new_data = {
        "Laptop": {"price": 1200, "cost": 800},
        "Phone": {"price": 800, "cost": 500},
        "Tablet": {"price": 600, "cost": 400},
    }

    with GridCraftClient() as client:
        print("🔄 Batch updating workbooks...")
        print("=" * 60)

        for wb in workbooks:
            print(f"\n📁 Processing {wb}...")

            try:
                client.open_workbook(wb)

                # Update pricing information
                for row in range(2, 102):  # Assuming data is in rows 2-101
                    product = client.evaluate_formula(f"=B{row}")
                    if product in new_data:
                        price = new_data[product]["price"]
                        cost = new_data[product]["cost"]
                        profit = price - cost

                        # Update price and profit columns
                        client.set_cell(f"G{row}", price, sheet="Sheet1")
                        client.set_cell(f"H{row}", cost, sheet="Sheet1")
                        client.set_cell(f"I{row}", profit, sheet="Sheet1")

                # Recalculate formulas
                client.execute_command("calc.recalculate", sheet="Sheet1")

                # Save updated workbook
                client.save_workbook(wb)
                print(f"  ✅ Updated: {wb}")

            except Exception as e:
                print(f"  ❌ Failed to update {wb}: {e}")

        print("\n✅ Batch update completed!")

    return 0


def main():
    print("🤖 GridCraft MCP Client - Automation Examples")
    print("=" * 60)

    try:
        # Example 1: Generate multiple reports
        print("\n📋 Example 1: Generate Monthly Reports")
        print("-" * 60)
        process_multiple_months()

        # Example 2: Batch update workbooks
        print("\n" + "=" * 60)
        print("\n🔄 Example 2: Batch Update Workbooks")
        print("-" * 60)
        batch_update_workbooks()

        print("\n" + "=" * 60)
        print("\n✨ All automation examples completed!")
        print("\n💡 Tips for production automation:")
        print("   - Use try/except blocks for error handling")
        print("   - Add logging for debugging")
        print("   - Consider using threading for parallel operations")
        print("   - Validate data before writing")
        print("   - Use environment variables for configuration")

    except Exception as e:
        print(f"\n❌ Error: {e}")
        import traceback

        traceback.print_exc()
        return 1

    return 0


if __name__ == "__main__":
    exit(main())
