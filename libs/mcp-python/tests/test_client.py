import pytest
from unittest.mock import MagicMock, patch
from gridcraft_mcp import GridCraftClient, ConnectionError, CommandError

@patch('subprocess.Popen')
class TestGridCraftClient:
    
    def test_client_initialization(self, mock_popen):
        client = GridCraftClient()
        assert client.port == 7979
    
    def test_start_success(self, mock_popen):
        client = GridCraftClient(_testing=True)
        with patch('socket.socket'):
            client.start()
        assert client.process is None # _testing=True, so Popen is never called
    
    def test_start_connection_failed(self, mock_popen):
        client = GridCraftClient(_testing=False)
        with patch('socket.socket') as mock_socket:
            mock_socket.return_value.connect.side_effect = ConnectionRefusedError
            with pytest.raises(ConnectionError):
                client.start()
    
    def test_stop(self, mock_popen):
        mock_socket = MagicMock()
        client = GridCraftClient(_socket=mock_socket)
        client.stop()
        mock_socket.close.assert_called_once()

@patch('gridcraft_mcp.client.GridCraftClient._send_request')
class TestCommands:
    def test_execute_command(self, mock_send):
        mock_send.return_value = {"status": "ok"}
        client = GridCraftClient(_socket=MagicMock())
        assert client.execute_command("cmd") == {"status": "ok"}
        mock_send.assert_called_once()

    def test_read_range(self, mock_send):
        mock_send.return_value = {"values": [["v"]]}
        client = GridCraftClient(_socket=MagicMock())
        assert client.read_range("A1") == [["v"]]
