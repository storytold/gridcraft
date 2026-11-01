#!/usr/bin/env python3
"""
Data analysis example using GridCraft MCP client.

This example demonstrates how to use GridCraft for data analysis tasks.
"""

from gridcraft_mcp import GridCraftClient
import random
from datetime import datetime, timedelta


def generate_sample_data():
    """Generate sample sales data"""
    products = ["Laptop", "Phone", "Tablet", "Monitor", "Keyboard"]
    regions = ["North", "South", "East", "West"]

    data = []
    data.append(["Date", "Product", "Region", "Sales", "Profit"])

    start_date = datetime(2024, 1, 1)

    for i in range(100):
        date = start_date + timedelta(days=i % 30)
        product = products[i % len(products)]
        region = regions[i % len(regions)]
        sales = random.randint(50, 500)
        profit = sales * random.uniform(0.2, 0.4)

        data.append([date.strftime("%Y-%m-%d"), product, region, sales, profit])

    return data


def main():
    print("📊 GridCraft MCP Client - Data Analysis Example")
    print("=" * 60)

    try:
        with GridCraftClient() as client:
            print("\n🚀 Starting data analysis workflow...")

            # Create new workbook
            print("\n📄 Creating new workbook...")
            client.new_workbook()
            print("✅ Workbook created")

            # Generate and write sample data
            print("\n📝 Generating sample sales data...")
            data = generate_sample_data()

            # Write headers
            client.write_range(
                "A1:E1",
                [[data[0][0]], [data[0][1]], [data[0][2]], [data[0][3]], [data[0][4]]],
            )

            # Write data rows
            for i, row in enumerate(data[1:], start=2):
                client.write_range(
                    f"A{i}:E{i}", [[row[0]], [row[1]], [row[2]], [row[3]], [row[4]]]
                )

            print(f"✅ Wrote {len(data)-1} rows of data")

            # Create a table
            print("\n📋 Creating structured table...")
            table_info = client.create_table(
                range_str="A1:E101", name="SalesData", has_headers=True
            )
            print(f"✅ Table created: {table_info.get('name')}")

            # Sort by sales (descending)
            print("\n🔽 Sorting data by sales (descending)...")
            client.sort_range(
                range_str="A1:E101",
                sort_by=[{"column": 3, "order": "descending"}],
                sheet="Sheet1",
            )
            print("✅ Data sorted")

            # Calculate statistics
            print("\n🧮 Calculating statistics...")

            total_sales = client.evaluate_formula("=SUM(D2:D101)")
            total_profit = client.evaluate_formula("=SUM(E2:E101)")
            avg_sales = client.evaluate_formula("=AVERAGE(D2:D101)")
            max_sales = client.evaluate_formula("=MAX(D2:D101)")
            min_sales = client.evaluate_formula("=MIN(D2:D101)")

            print("\n📈 Sales Statistics:")
            print("  Total Sales: ${:,.2f}".format(total_sales))
            print("  Total Profit: ${:,.2f}".format(total_profit))
            print("  Average Sale: ${:,.2f}".format(avg_sales))
            print("  Maximum Sale: ${:,.2f}".format(max_sales))
            print("  Minimum Sale: ${:,.2f}".format(min_sales))

            # Top 5 products by sales
            print("\n🏆 Top 5 Products by Sales:")
            top_products = client.evaluate_formula(
                "=SORT(UNIQUE(A2:A101), -COUNTIF(A2:A101, UNIQUE(A2:A101)), 1)"
            )
            print(f"  {top_products}")

            # Sales by region
            print("\n🗺️ Sales by Region:")
            region_sales = client.evaluate_formula(
                "=SUMIFS(D2:D101, C2:C101, UNIQUE(C2:C101))"
            )
            print(f"  {region_sales}")

            # Apply conditional formatting
            print("\n🎨 Applying conditional formatting...")

            # Highlight top 10 sales
            client.format_range(
                "D2:D101",
                "conditional",
                {"type": "top_n", "value": 10, "format": "bold", "color": "green"},
            )

            # Highlight losses (profit < 0)
            client.format_range(
                "E2:E101",
                "conditional",
                {
                    "type": "cell_value",
                    "operator": "less_than",
                    "value": 0,
                    "format": "red_bold",
                },
            )

            print("✅ Conditional formatting applied")

            # Create summary table
            print("\n📊 Creating summary table...")

            # Write summary headers
            client.write_range("G1:H1", [["Metric"], ["Value"]])

            # Write summary data
            metrics = [
                ["Total Sales", f"${total_sales:,.2f}"],
                ["Total Profit", f"${total_profit:,.2f}"],
                ["Average Sale", f"${avg_sales:,.2f}"],
                ["Max Sale", f"${max_sales:,.2f}"],
                ["Min Sale", f"${min_sales:,.2f}"],
            ]

            for i, (metric, value) in enumerate(metrics, start=2):
                client.write_range(f"G{i}:H{i}", [[metric], [value]])

            # Format summary
            client.format_range("G1:H6", "bold")
            client.format_range("H2:H6", "currency")

            print("✅ Summary table created")

            # Create charts
            print("\n📈 Creating charts...")

            # Sales by product chart
            chart1 = client.insert_chart(
                range_str="A1:E101",
                chart_type="column",
                title="Sales by Product",
                x_axis="Product",
                y_axis="Sales ($)",
            )
            print(f"✅ Chart 1 created: {chart1.get('id')}")

            # Profit by region chart
            chart2 = client.insert_chart(
                range_str="A1:E101",
                chart_type="pie",
                title="Profit by Region",
                x_axis="Region",
                y_axis="Profit ($)",
            )
            print(f"✅ Chart 2 created: {chart2.get('id')}")

            # Export to CSV
            print("\n💾 Exporting data...")
            client.export_csv("A1:E101", "sales_analysis.csv")
            print("✅ Data exported to 'sales_analysis.csv'")

            # Save workbook
            timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
            filename = f"sales_analysis_{timestamp}.xlsx"
            client.save_workbook(filename)
            print(f"✅ Workbook saved as '{filename}'")

            print("\n✅ Data analysis workflow completed successfully!")

    except Exception as e:
        print(f"\n❌ Error: {e}")
        import traceback

        traceback.print_exc()
        return 1

    return 0


if __name__ == "__main__":
    exit(main())
