import random
from gridcraft_mcp import GridCraftClient


def generate_financial_data():
    categories = ["Q1 2024", "Q2 2024", "Q3 2024", "Q4 2024"]
    revenue_data = []
    expenses_data = []
    profit_data = []
    for category in categories:
        revenue = random.randint(100000, 500000)
        expenses = random.randint(50000, 300000)
        profit = revenue - expenses
        revenue_data.append(revenue)
        expenses_data.append(expenses)
        profit_data.append(profit)
    return {
        "categories": categories,
        "revenue": revenue_data,
        "expenses": expenses_data,
        "profit": profit_data,
    }


def main():
    print("📊 GridCraft MCP Client - Charts and Tables Example")
    print("=" * 60)
    try:
        with GridCraftClient(port=7979) as client:
            print("\n🚀 Starting charts and tables demonstration...")
            client.new_workbook()
            data = generate_financial_data()
            client.set_cell("A1", "Quarter")
            client.set_cell("B1", "Revenue")
            client.set_cell("C1", "Expenses")
            client.set_cell("D1", "Profit")
            for i, (q, r, e, p) in enumerate(
                zip(
                    data["categories"],
                    data["revenue"],
                    data["expenses"],
                    data["profit"],
                ),
                start=2,
            ):
                client.set_cell(f"A{i}", q)
                client.set_cell(f"B{i}", r)
                client.set_cell(f"C{i}", e)
                client.set_cell(f"D{i}", p)
            client.create_table(
                range_str="A1:D6", name="FinancialData", has_headers=True
            )
            client.format_range("B2:D6", "currency")
            client.format_range("A1:D1", "bold")
            client.format_range("A1:D1", "fill", {"color": "#DDEBF7"})
            print("\n🖼️ Creating dashboard layout...")
            client.set_cell("A8", "Financial Dashboard")
            client.format_range("A8:D8", "bold")
            client.format_range("A8:D8", "font_size", 16)
            client.set_cell("A9", "Total Revenue")
            client.set_cell("B9", "=SUM(B2:B5)")
            print("\n✅ Charts and tables demonstration completed successfully!")
    except Exception as e:
        print(f"\n❌ Error: {e}")
        return 1
    return 0


if __name__ == "__main__":
    main()
