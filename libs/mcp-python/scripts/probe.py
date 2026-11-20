from gridcraft_mcp import GridCraftClient


def probe():
    try:
        with GridCraftClient(port=7979) as client:
            commands_to_try = ["list_commands", "list", "tools/list", "get_commands"]

            for cmd in commands_to_try:
                print(f"--- Trying: {cmd} ---")
                try:
                    print(f"Result: {client.execute_command(cmd)}")
                except Exception as e:
                    print(f"Failed: {e}")
    except Exception as e:
        print(f"Connection error: {e}")


if __name__ == "__main__":
    probe()
