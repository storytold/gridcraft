#!/usr/bin/env python3
"""
Basic usage example for GridCraft MCP client.

This example demonstrates fundamental operations with GridCraft.
"""

from gridcraft_mcp import GridCraftClient


def main():
    print("🚀 GridCraft MCP Client - Basic Usage Example")
    print("=" * 60)

    try:
        # Example 1: Auto-start GridCraft with context manager
        print("\n📝 Example 1: Auto-start GridCraft")
        print("-" * 40)

        with GridCraftClient() as client:
            print("✅ GridCraft started successfully!")

            # Create new workbook
            print("\n📄 Creating new workbook...")
            client.new_workbook()
            print("✅ New workbook created")

            # Write some data
            print("\n📝 Writing data to cells...")
            client.set_cell("A1", "Product")
            client.set_cell("B1", "Sales")
            client.set_cell("C1", "Profit")

            client.set_cell("A2", "Widget A")
            client.set_cell("B2", 100)
            client.set_cell("C2", 30)

            client.set_cell("A3", "Widget B")
            client.set_cell("B3", 150)
            client.set_cell("C3", 45)

            client.set_cell("A4", "Widget C")
            client.set_cell("B4", 200)
            client.set_cell("C4", 60)

            print("✅ Data written successfully")

            # Read data back
            print("\n📖 Reading data from sheet...")
            data = client.read_range("A1:C4")
            print("\n📊 Sheet Content:")
            for row in data:
                print(f"  {row}")

            # Calculate totals
            print("\n🧮 Calculating totals...")
            total_sales = client.evaluate_formula("=SUM(B2:B4)")
            total_profit = client.evaluate_formula("=SUM(C2:C4)")
            avg_profit = client.evaluate_formula("=AVERAGE(C2:C4)")

            print(f"  Total Sales: ${total_sales:,.2f}")
            print(f"  Total Profit: ${total_profit:,.2f}")
            print(f"  Average Profit: ${avg_profit:,.2f}")

            # Apply formatting
            print("\n🎨 Applying formatting...")
            client.format_range("B2:B4", "currency")
            client.format_range("C2:C4", "currency")
            print("✅ Formatting applied")

            # Create a chart
            print("\n📊 Creating chart...")
            chart_info = client.insert_chart(
                range_str="A1:C4",
                chart_type="column",
                title="Sales and Profit by Product",
                x_axis="Product",
                y_axis="Amount ($)",
            )
            print(f"✅ Chart created: {chart_info.get('id', 'unknown')}")

            # Save workbook
            print("\n💾 Saving workbook...")
            client.save_workbook("basic_example.xlsx")
            print("✅ Workbook saved as 'basic_example.xlsx'")

        print("\n✅ All operations completed successfully!")

        # Example 2: Connect to existing instance
        print("\n" + "=" * 60)
        print("\n🔌 Example 2: Connect to existing GridCraft instance")
        print("-" * 60)

        # This would connect to an already running GridCraft instance
        # client = GridCraftClient(port=7979)
        # client.open_workbook("existing_file.xlsx")
        # ... work with the file ...
        # client.stop()

        print("📝 To connect to existing instance:")
        print("   client = GridCraftClient(port=7979)")
        print("   # ... work with file ...")
        print("   client.stop()")

    except Exception as e:
        print(f"\n❌ Error: {e}")
        import traceback

        traceback.print_exc()
        return 1

    return 0


if __name__ == "__main__":
    exit(main())
