#!/bin/bash

# Visual Demo Script for GridCraft MCP Python Client

CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() { echo -e "${CYAN}>>> ${1}${NC}"; }
warn() { echo -e "${YELLOW}⚠ ${1}${NC}"; }

# Check whether GridCraft is running
if ! ss -tuln 2>/dev/null | grep -q ":7979 "; then
    warn "GridCraft was not detected on port 7979."
    echo ""
    log "Please start GridCraft in another terminal:"
    echo -e "${CYAN}    gridcraft --control 7979${NC}"
    echo ""
    read -p "Press Enter once the application is running..."
fi

# Resolve the absolute path of the scripts folder
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Add the module path (the libs/mcp-python folder) to PYTHONPATH
export PYTHONPATH="$SCRIPT_DIR/.."

log "Running the Python demo..."
python3 "$SCRIPT_DIR/demo_visual.py"
