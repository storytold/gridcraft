#!/bin/bash

# GridCraft MCP Client - Test Runner Script
#
# This script runs all tests for the GridCraft MCP Python client
# and displays the results in a user-friendly format.
#
# Features:
# - Automatic dependency installation
# - GridCraft CLI installation (optional)
# - Test execution with coverage
# - Error handling and user-friendly output

set -eo pipefail

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PYTHON_CMD="python3"
PIP_CMD="pip3"
CARGO_CMD="cargo"

# GridCraft CLI configuration
gridcraft_cli_installed=false
gridcraft_cli_path=""

# Logging function
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[✓]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[⚠]${NC} $1"
}

log_error() {
    echo -e "${RED}[✗]${NC} $1" >&2
}

log_question() {
    echo -ne "${PURPLE}[?]${NC} $1"
}

log_highlight() {
    echo -e "${CYAN}>>> $1${NC}"
}

# Display header
show_header() {
    echo ""
    echo "╔════════════════════════════════════════════════════════════╗"
    echo "║     GridCraft MCP Client - Automated Test Runner          ║"
    echo "╚════════════════════════════════════════════════════════════╝"
    echo ""
}

# Check if dependencies are installed
check_dependencies() {
    log_info "Checking dependencies..."
    
    # Check Python
    if ! command -v $PYTHON_CMD &> /dev/null; then
        log_error "Python3 is not installed. Please install Python 3.8+"
        log_highlight "On Ubuntu/Debian: sudo apt install python3 python3-pip"
        log_highlight "On macOS: brew install python"
        log_highlight "On Windows: Download from python.org"
        exit 1
    fi
    
    # Check pip
    if ! command -v $PIP_CMD &> /dev/null; then
        log_error "pip3 is not installed"
        log_highlight "Install pip with: sudo apt install python3-pip"
        exit 1
    fi
    
    # Check pytest
    if ! $PYTHON_CMD -m pytest --version &> /dev/null; then
        log_warning "pytest is not installed. Installing..."
        
        # Try to install with --user flag first
        if $PIP_CMD install --user pytest pytest-cov 2>/dev/null; then
            log_success "pytest installed in user space"
        else
            # If that fails, try with --break-system-packages
            if $PIP_CMD install --user --break-system-packages pytest pytest-cov 2>/dev/null; then
                log_success "pytest installed with --break-system-packages flag"
            else
                log_error "Failed to install pytest"
                log_highlight "Try: pip install --user pytest pytest-cov"
                log_highlight "Or create a virtual environment: python3 -m venv venv && source venv/bin/activate && pip install pytest"
                exit 1
            fi
        fi
    fi
    
    log_success "Dependencies check passed"
}

# Check if GridCraft CLI is available
check_gridcraft_cli() {
    local default_path="$PROJECT_DIR/../target/release/gridcraft-cli"
    
    # Check if already found
    if [ "$gridcraft_cli_installed" = true ]; then
        log_success "GridCraft CLI found at: $gridcraft_cli_path"
        return 0
    fi
    
    # Check in PATH
    if command -v gridcraft-cli &> /dev/null; then
        gridcraft_cli_path=$(which gridcraft-cli)
        gridcraft_cli_installed=true
        log_success "GridCraft CLI found in PATH: $gridcraft_cli_path"
        return 0
    fi
    
    # Check in default build location
    if [ -f "$default_path" ]; then
        gridcraft_cli_path="$default_path"
        gridcraft_cli_installed=true
        log_success "GridCraft CLI found at: $gridcraft_cli_path"
        return 0
    fi
    
    log_warning "GridCraft CLI not found"
    return 1
}

# Install GridCraft CLI from source
install_gridcraft_cli() {
    log_info "GridCraft CLI not found. Installing from source..."
    
    # Check if cargo is available
    if ! command -v $CARGO_CMD &> /dev/null; then
        log_error "Cargo (Rust toolchain) is not installed. Cannot build GridCraft CLI"
        log_highlight "Install Rust: https://www.rust-lang.org/tools/install"
        log_highlight "Then install GridCraft from source"
        return 1
    fi
    
    # Navigate to GridCraft repository root
    local gridcraft_root="$PROJECT_DIR/../"
    
    if [ ! -f "$gridcraft_root/Cargo.toml" ]; then
        log_error "GridCraft repository not found at: $gridcraft_root"
        log_highlight "Clone GridCraft first:"
        log_highlight "  git clone https://github.com/storytold/gridcraft.git"
        log_highlight "  cd gridcraft"
        return 1
    fi
    
    log_info "Building GridCraft CLI..."
    
    # Build GridCraft CLI
    cd "$gridcraft_root"
    
    if $CARGO_CMD build --release -p gridcraft-cli 2>/dev/null; then
        gridcraft_cli_path="$gridcraft_root/target/release/gridcraft-cli"
        gridcraft_cli_installed=true
        log_success "GridCraft CLI built successfully!"
        log_highlight "Binary location: $gridcraft_cli_path"
        
        # Add to PATH temporarily
        export PATH="$gridcraft_root/target/release:$PATH"
        export GRIDCRAFT_CLI_PATH="$gridcraft_cli_path"
        
        return 0
    else
        log_error "Failed to build GridCraft CLI"
        log_highlight "Check Rust installation and try building manually:"
        log_highlight "  cd $gridcraft_root && cargo build --release -p gridcraft-cli"
        return 1
    fi
}

# Run unit tests
run_unit_tests() {
    log_info "Running unit tests..."
    
    cd "$PROJECT_DIR"
    
    # Ensure pytest can find the module
    export PYTHONPATH="${PROJECT_DIR}:/usr/local/lib/python3.12/site-packages:$PYTHONPATH"
    
    $PYTHON_CMD -m pytest tests/test_client.py tests/test_examples.py -v --tb=short
    
    log_success "Unit tests completed successfully"
}

# Run integration tests
run_integration_tests() {
    if ! check_gridcraft_cli; then
        log_warning "Skipping integration tests - GridCraft CLI not available"
        return 0
    fi
    
    log_info "Running integration tests..."
    
    cd "$PROJECT_DIR"
    
    # Set default port if not specified
    export GRIDCRAFT_PORT="${GRIDCRAFT_PORT:-7979}"
    # Ensure pytest can find the module
    export PYTHONPATH="${PROJECT_DIR}:/usr/local/lib/python3.12/site-packages:$PYTHONPATH"
    
    $PYTHON_CMD -m pytest tests/test_integration.py -v --tb=short
    
    log_success "Integration tests completed successfully"
}

# Run example validation tests
run_example_tests() {
    log_info "Running example validation tests..."
    
    cd "$PROJECT_DIR"
    
    # Ensure pytest can find the module
    export PYTHONPATH="${PROJECT_DIR}:/usr/local/lib/python3.12/site-packages:$PYTHONPATH"
    
    $PYTHON_CMD -m pytest tests/test_examples.py -v --tb=short
    
    log_success "Example validation tests completed successfully"
}

# Run all tests
run_all_tests() {
    log_info "Running ALL tests..."
    
    # Run unit tests first
    run_unit_tests
    
    # Run example tests
    run_example_tests
    
    # Run integration tests if available
    if check_gridcraft_cli; then
        run_integration_tests
    fi
    
    log_success "All tests completed successfully!"
}

# Run tests with coverage
run_coverage_tests() {
    log_info "Running tests with coverage..."
    
    cd "$PROJECT_DIR"
    
    # Ensure pytest can find the module
    export PYTHONPATH="${PROJECT_DIR}:/usr/local/lib/python3.12/site-packages:$PYTHONPATH"
    
    $PYTHON_CMD -m pytest tests/ --cov=gridcraft_mcp --cov-report=term-missing -v --tb=short
    
    log_success "Coverage tests completed"
    
    # Generate HTML report
    log_info "Generating HTML coverage report..."
    $PYTHON_CMD -m pytest tests/ --cov=gridcraft_mcp --cov-report=html
    
    local coverage_file="$PROJECT_DIR/htmlcov/index.html"
    if [ -f "$coverage_file" ]; then
        log_success "Coverage report generated: $coverage_file"
        log_highlight "Open it in your browser or run: xdg-open $coverage_file"
    fi
}

# Clean test artifacts
clean_tests() {
    log_info "Cleaning test artifacts..."
    
    cd "$PROJECT_DIR"
    
    # Remove coverage reports
    if [ -d "htmlcov" ]; then
        rm -rf htmlcov
        log_success "Removed htmlcov directory"
    fi
    
    # Remove pytest cache
    if [ -d ".pytest_cache" ]; then
        rm -rf .pytest_cache
        log_success "Removed .pytest_cache directory"
    fi
    
    # Remove coverage files
    find . -name ".coverage*" -delete 2>/dev/null || true
    find . -name "*.pyc" -delete 2>/dev/null || true
    find . -name "__pycache__" -type d -exec rm -rf {} + 2>/dev/null || true
    
    log_success "Test artifacts cleaned"
}

# Display test summary
show_summary() {
    echo ""
    echo "╔════════════════════════════════════════════════════════════╗"
    echo "║           GridCraft MCP Client - Test Summary            ║"
    echo "╚════════════════════════════════════════════════════════════╝"
    echo ""
    
    echo "📊 Test Coverage:"
    echo "   • Unit Tests: 30+ tests (client initialization, methods, error handling)"
    echo "   • Integration Tests: 25+ tests (real GridCraft interaction)"
    echo "   • Example Tests: 5+ tests (syntax validation)"
    echo "   • Total: 60+ tests"
    echo ""
    
    echo "🚀 Available Commands:"
    echo "   ./scripts/run_tests.sh unit          - Run unit tests only"
    echo "   ./scripts/run_tests.sh integration   - Run integration tests only"
    echo "   ./scripts/run_tests.sh examples      - Run example validation"
    echo "   ./scripts/run_tests.sh all           - Run all tests"
    echo "   ./scripts/run_tests.sh coverage      - Run tests with coverage report"
    echo "   ./scripts/run_tests.sh install       - Install GridCraft CLI"
    echo "   ./scripts/run_tests.sh clean         - Clean test artifacts"
    echo ""
    
    echo "📝 Notes:"
    echo "   • Integration tests require GridCraft CLI with MCP server"
    echo "   • Use './scripts/run_tests.sh install' to install GridCraft CLI"
    echo "   • Set GRIDCRAFT_CLI_PATH to specify gridcraft-cli location"
    echo "   • Coverage report is saved to htmlcov/index.html"
    echo "   • If you get PEP 668 errors, create a virtual environment"
    echo "     python3 -m venv venv && source venv/bin/activate && pip install -e \".[dev]\""
    echo ""
    
    if [ "$gridcraft_cli_installed" = true ]; then
        echo "🎉 GridCraft CLI is installed and ready for integration tests!"
    else
        echo "⚠️  GridCraft CLI not installed. Integration tests will be skipped."
        echo "   Run './scripts/run_tests.sh install' to install it."
    fi
    echo ""
}

# Main execution
main() {
    show_header
    
    # Check command line arguments
    case "${1:-all}" in
        "unit")
            check_dependencies
            run_unit_tests
            ;;
        
        "integration")
            check_dependencies
            
            # Try to install GridCraft CLI if not found
            if ! check_gridcraft_cli; then
                if install_gridcraft_cli; then
                    check_gridcraft_cli || exit 1
                else
                    log_warning "Skipping integration tests - GridCraft CLI installation failed"
                    exit 0
                fi
            fi
            
            run_integration_tests
            ;;
        
        "examples")
            check_dependencies
            run_example_tests
            ;;
        
        "all")
            check_dependencies
            
            # Try to install GridCraft CLI for full testing
            if ! check_gridcraft_cli; then
                log_warning "GridCraft CLI not found. Integration tests will be skipped."
                log_question "Would you like to install GridCraft CLI now? [y/N] "
                read -r answer
                if [[ "$answer" =~ ^[Yy]$ ]]; then
                    if install_gridcraft_cli; then
                        check_gridcraft_cli || exit 1
                    else
                        log_warning "GridCraft CLI installation failed. Continuing without it."
                    fi
                fi
            fi
            
            run_all_tests
            ;;
        
        "coverage")
            check_dependencies
            run_coverage_tests
            ;;
        
        "install")
            check_dependencies
            install_gridcraft_cli
            ;;
        
        "clean")
            clean_tests
            ;;
        
        "")
            check_dependencies
            
            # Try to install GridCraft CLI for full testing
            if ! check_gridcraft_cli; then
                log_warning "GridCraft CLI not found. Integration tests will be skipped."
                log_question "Would you like to install GridCraft CLI now? [y/N] "
                read -r answer
                if [[ "$answer" =~ ^[Yy]$ ]]; then
                    if install_gridcraft_cli; then
                        check_gridcraft_cli || exit 1
                    else
                        log_warning "GridCraft CLI installation failed. Continuing without it."
                    fi
                fi
            fi
            
            run_all_tests
            ;;
        
        *)
            log_error "Unknown command: $1"
            echo ""
            show_summary
            exit 1
            ;;
    esac
    
    # Always show summary
    show_summary
    
    exit 0
}

# Execute main function
main "$@"
