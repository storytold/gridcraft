# Installation Guide for GridCraft MCP Client

This guide helps you set up the GridCraft MCP Python client and its dependencies.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Installation Methods](#installation-methods)
- [Troubleshooting](#troubleshooting)
- [Virtual Environment Setup](#virtual-environment-setup)
- [System-wide Installation](#system-wide-installation)
- [Docker Setup](#docker-setup)

---

## Prerequisites

### Required Software

1. **Python 3.8+**
   - Python 3.8, 3.9, 3.10, 3.11, or 3.12
   
2. **pip** (Python package manager)
   - Usually comes with Python
   
3. **Git** (for cloning repository)
   - [Download Git](https://git-scm.com/downloads)

### Optional but Recommended

1. **GridCraft CLI** (for integration tests)
   - Build from source or download from [GridCraft releases](https://github.com/storytold/gridcraft/releases)

2. **Virtual environment** (recommended)
   - Isolates dependencies from system Python

---

## Installation Methods

### Method 1: Using Virtual Environment (Recommended)

**Best for:** Development, testing, avoiding system conflicts

```bash
# 1. Clone the repository
cd /path/to/parent/directory
git clone https://github.com/storytold/gridcraft.git
cd gridcraft/gridcraft-python

# 2. Create virtual environment
python3 -m venv venv

# 3. Activate virtual environment
# Linux/macOS:
source venv/bin/activate

# Windows:
# venv\Scripts\activate

# 4. Install the package in development mode
pip install -e ".[dev]"

# 5. Verify installation
python -c "from gridcraft_mcp import GridCraftClient; print('✓ GridCraft MCP client installed')"

# 6. Run tests
./scripts/run_tests.sh
```

**Benefits:**
- ✅ Isolated dependencies
- ✅ No conflicts with system Python
- ✅ Easy to clean up
- ✅ Works on all platforms

---

### Method 2: System-wide Installation

**Best for:** Quick testing, CI/CD systems

```bash
# Clone the repository
cd /path/to/parent/directory
git clone https://github.com/storytold/gridcraft.git
cd gridcraft/gridcraft-python

# Install with --user flag (avoids sudo)
pip install --user -e ".[dev]"

# Add user bin to PATH (if needed)
export PATH="$HOME/.local/bin:$PATH"

# Verify installation
python -c "from gridcraft_mcp import GridCraftClient; print('✓ GridCraft MCP client installed')"
```

**Note:** On Debian/Ubuntu with Python 3.12+, you may need to use `--break-system-packages`:

```bash
pip install --user --break-system-packages -e ".[dev]"
```

---

### Method 3: Using pipx (Recommended for CLI tools)

**Best for:** Installing as a CLI tool

```bash
# Install pipx
python3 -m pip install --user pipx
python3 -m pipx ensurepath

# Clone the repository
cd /path/to/parent/directory
git clone https://github.com/storytold/gridcraft.git
cd gridcraft/gridcraft-python

# Install with pipx
pipx install -e .

# Verify installation
gridcraft-mcp-client --help  # If you add a CLI entry point
```

**Benefits:**
- ✅ Isolated installation
- ✅ No conflicts with other Python packages
- ✅ Easy to upgrade/uninstall

---

### Method 4: Docker Setup

**Best for:** Containerized environments, CI/CD, reproducible builds

```bash
# 1. Build Docker image
cd /path/to/gridcraft/gridcraft-python
docker build -t gridcraft-mcp-client .

# 2. Run tests in container
docker run --rm -it gridcraft-mcp-client ./scripts/run_tests.sh

# 3. Run interactive shell
docker run --rm -it gridcraft-mcp-client bash
```

**Dockerfile Example:**
```dockerfile
FROM python:3.10-slim

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy project files
COPY . .

# Install Python dependencies
RUN pip install --no-cache-dir -e ".[dev]"

# Set entry point
ENTRYPOINT ["./scripts/run_tests.sh"]
```

---

## Troubleshooting

### Common Issues and Solutions

#### Issue 1: PEP 668 Error (Debian/Ubuntu)

**Error:**
```
× This environment is externally managed
╰─> To install Python packages system-wide, try apt install...
```

**Solutions:**

**Option A: Use virtual environment (Recommended)**
```bash
python3 -m venv venv
source venv/bin/activate
pip install -e ".[dev]"
```

**Option B: Use --break-system-packages flag**
```bash
pip install --user --break-system-packages -e ".[dev]"
```

**Option C: Install system packages**
```bash
sudo apt install python3-pip python3-venv
```

---

#### Issue 2: pytest not found

**Error:**
```
pytest: command not found
```

**Solutions:**

```bash
# Install pytest
pip install pytest

# Or with virtual environment
python3 -m venv venv
source venv/bin/activate
pip install pytest
```

---

#### Issue 3: GridCraft CLI not found

**Error:**
```
GridCraft CLI not found in PATH
```

**Solutions:**

**Option A: Build GridCraft from source**
```bash
# From GridCraft repository root
cd gridcraft
cargo build --release -p gridcraft-cli

# Add to PATH
export PATH="$PWD/target/release:$PATH"
```

**Option B: Download pre-built binary**
```bash
# Download from GitHub releases
wget https://github.com/storytold/gridcraft/releases/download/vX.Y.Z/gridcraft-cli-linux-x86_64
chmod +x gridcraft-cli-linux-x86_64
mv gridcraft-cli-linux-x86_64 /usr/local/bin/gridcraft-cli
```

**Option C: Set custom path**
```bash
export GRIDCRAFT_CLI_PATH=/path/to/your/gridcraft-cli
```

---

#### Issue 4: Permission denied errors

**Error:**
```
Permission denied: /usr/local/lib/python3.10/site-packages/...
```

**Solutions:**

```bash
# Use --user flag
pip install --user package_name

# Or use virtual environment
python3 -m venv venv
source venv/bin/activate
pip install package_name
```

---

#### Issue 5: Python version too old

**Error:**
```
Python 3.8+ required
```

**Solutions:**

**Option A: Upgrade Python**
```bash
# Ubuntu/Debian
sudo apt update
sudo apt install python3.10 python3.10-venv python3.10-dev

# macOS
brew install python@3.10

# Windows
Download from python.org
```

**Option B: Use pyenv (multiple Python versions)**
```bash
# Install pyenv
curl https://pyenv.run | bash

# Install specific Python version
pyenv install 3.10.12
pyenv global 3.10.12
```

---

## Virtual Environment Setup

### Creating a Virtual Environment

```bash
# Navigate to project directory
cd /path/to/gridcraft/gridcraft-python

# Create virtual environment
python3 -m venv venv

# Activate (Linux/macOS)
source venv/bin/activate

# Activate (Windows)
# venv\Scripts\activate

# Deactivate when done
deactivate
```

### Virtual Environment Management

```bash
# List installed packages
pip list

# Freeze dependencies
pip freeze > requirements.txt

# Install from requirements
pip install -r requirements.txt

# Delete virtual environment
rm -rf venv
```

---

## System-wide Installation

### On Linux/macOS

```bash
# Install system dependencies
sudo apt update
sudo apt install python3 python3-pip python3-venv git

# Clone repository
git clone https://github.com/storytold/gridcraft.git
cd gridcraft/gridcraft-python

# Install with --user flag
pip install --user -e ".[dev]"

# Add to PATH (if needed)
export PATH="$HOME/.local/bin:$PATH"
```

### On Windows

```powershell
# Install Python from python.org
# Make sure to check "Add Python to PATH" during installation

# Open Command Prompt as Administrator
cd C:\path\to\gridcraft\gridcraft-python

# Create virtual environment
python -m venv venv

# Activate
venv\Scripts\activate

# Install
pip install -e ".[dev]"
```

---

## Docker Setup

### Building Docker Image

```bash
# Navigate to project directory
cd /path/to/gridcraft/gridcraft-python

# Build image
docker build -t gridcraft-mcp-client .

# Run container
docker run --rm -it gridcraft-mcp-client

# Run tests in container
docker run --rm -it gridcraft-mcp-client ./scripts/run_tests.sh

# Mount local directory for development
# (Useful for editing files)
docker run --rm -it -v "$(pwd):/app" gridcraft-mcp-client bash
```

### Docker Compose

```yaml
version: '3.8'

services:
  gridcraft-tests:
    build: .
    volumes:
      - .:/app
    working_dir: /app
    command: ./scripts/run_tests.sh
```

Then run:
```bash
docker-compose run gridcraft-tests
```

---

## Verification

### Check Installation

```bash
# Check Python package
python -c "from gridcraft_mcp import GridCraftClient; print('✓ Package imported successfully')"

# Check version
python -c "import gridcraft_mcp; print(f'Version: {gridcraft_mcp.__version__}')"

# Check CLI
which gridcraft-cli || echo "GridCraft CLI not in PATH"
```

### Run Sample Code

```python
from gridcraft_mcp import GridCraftClient

# Test basic functionality
with GridCraftClient() as client:
    client.new_workbook()
    client.set_cell("A1", "Hello, GridCraft!")
    data = client.read_range("A1:A1")
    print(f"✓ Data: {data}")
```

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `GRIDCRAFT_CLI_PATH` | Path to gridcraft-cli executable | `gridcraft-cli` |
| `GRIDCRAFT_PORT` | Port for MCP server | `7979` |
| `PYTHONPATH` | Additional Python module paths | (system default) |

**Example:**
```bash
export GRIDCRAFT_CLI_PATH=/usr/local/bin/gridcraft-cli
export GRIDCRAFT_PORT=8080
export PYTHONPATH=/custom/path:$PYTHONPATH
```

---

## Updating Dependencies

```bash
# Update pip
pip install --upgrade pip

# Update all dependencies
pip install --upgrade -e ".[dev]"

# Or update specific package
pip install --upgrade pytest
```

---

## Uninstalling

### From Virtual Environment

```bash
# Deactivate
source venv/bin/deactivate  # or venv\Scripts\deactivate on Windows

# Delete virtual environment
rm -rf venv
```

### System-wide Uninstall

```bash
# Uninstall package
pip uninstall gridcraft-mcp

# Remove user installation
pip uninstall --user gridcraft-mcp

# Remove GridCraft CLI
sudo rm /usr/local/bin/gridcraft-cli
```

---

## Support

If you're still having issues:

1. **Check the error message carefully**
2. **Read the full error output**
3. **Try a different installation method**
4. **Create an issue on GitHub**
5. **Ask on Discord** - [https://discord.gg/artcraft](https://discord.gg/artcraft)

---

## Additional Resources

- [Python Official Documentation](https://docs.python.org/3/)
- [pip User Guide](https://pip.pypa.io/)
- [Virtual Environment Guide](https://docs.python.org/3/tutorial/venv.html)
- [PEP 668 - Marking Python base environments as "externally managed"](https://peps.python.org/pep-0668/)

---

## Summary

| Method | Pros | Cons |
|--------|------|------|
| **Virtual Environment** | ✅ Isolated, ✅ Recommended | ⚠️ Slightly more setup |
| **System-wide** | ✅ Simple | ❌ May conflict with system Python |
| **pipx** | ✅ Isolated CLI tool | ❌ Not ideal for development |
| **Docker** | ✅ Reproducible, ✅ Containerized | ⚠️ Requires Docker |

**Recommendation:** Use **Virtual Environment** for development and testing.

---

**Need help?** Check the [GridCraft Discord](https://discord.gg/artcraft) or open an issue on GitHub!
