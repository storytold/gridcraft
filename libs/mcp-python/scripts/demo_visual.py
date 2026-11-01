from gridcraft_mcp import GridCraftClient


def run_demo():
    print("🤖 [Python] Connecting to GridCraft...")
    try:
        with GridCraftClient(port=7979) as client:
            print("🤖 [Python] Step 1: New workbook")
            client.new_workbook()

            print("🤖 [Python] Step 2: Writing data (A1, B1, A2, B2)")
            client.set_cell("A1", "Product")
            client.set_cell("B1", "Price")
            client.set_cell("A2", "GridCraft")
            client.set_cell("B2", 5000)

            print("🤖 [Python] Step 3: Reading cell B2")
            val = client.get_cell("B2")
            print(f"   Data received: {val}")

            print("🤖 [Python] Demo finished!")
    except Exception as e:
        print(f"❌ Connection error: {e}")
        print("   Make sure GridCraft is running with the --control 7979 flag")


if __name__ == "__main__":
    run_demo()
