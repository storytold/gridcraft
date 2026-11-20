import socket
import json
import subprocess
from typing import List, Dict, Any, Optional
from dataclasses import dataclass
from gridcraft_mcp.exceptions import ConnectionError, CommandError


@dataclass
class GridCraftClient:
    port: int = 7979
    host: str = "localhost"
    process: Optional[subprocess.Popen] = None
    _request_id: int = 1
    _socket: Optional[socket.socket] = None
    _testing: bool = False

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.stop()

    def start(self) -> None:
        if not self._socket:
            self._socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            if not self._testing:
                try:
                    self._socket.connect((self.host, self.port))
                except ConnectionRefusedError:
                    raise ConnectionError(
                        f"Could not connect to GridCraft at {self.host}:{self.port}"
                    )

    def stop(self) -> None:
        if self._socket:
            self._socket.close()
            self._socket = None

    def _send_request(self, method: str, params: Dict[str, Any] = None) -> Any:
        if not self._socket:
            raise ConnectionError("Not connected")

        payload = {
            "jsonrpc": "2.0",
            "method": "engine.execute",
            "params": {"command": method, "params": params or {}},
            "id": self._request_id,
        }
        self._request_id += 1

        try:
            self._socket.sendall((json.dumps(payload) + "\n").encode("utf-8"))

            response_data = b""
            while b"\n" not in response_data:
                chunk = self._socket.recv(4096)
                if not chunk:
                    break
                response_data += chunk

            response = json.loads(response_data.decode("utf-8"))
            if "error" in response:
                raise CommandError(str(response["error"]), method, response)
            return response.get("result", response)
        except Exception as e:
            if isinstance(e, CommandError):
                raise
            raise ConnectionError(f"RPC communication failed: {e}")

    def execute_command(self, command: str, **params) -> Dict[str, Any]:
        return self._send_request(command, params)

    def new_workbook(self) -> bool:
        return bool(self.execute_command("file.new"))

    def set_cell(self, cell: str, value: Any) -> bool:
        return bool(self.execute_command("cell.set", cell=cell, input=str(value)))

    def get_cell(self, cell: str) -> Any:
        res = self.execute_command("cell.get", cell=cell)
        return res.get("value")

    def read_range(self, range_str: str) -> List[List[Any]]:
        res = self.execute_command("read_range", range=range_str)
        return res.get("values", []) if res else []
