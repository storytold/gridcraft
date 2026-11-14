#!/bin/bash

# GridCraft MCP Client - CI/CD Runner Script
#
# This script runs all CI checks for the GridCraft MCP Python client
# and is designed to be used in GitHub Actions or other CI/CD systems.
#
# Usage:
#   ./scripts/run_ci.sh              # Run all CI checks
#   ./scripts/run_ci.sh test         # Run only tests
#   ./scripts/run_ci.sh lint         # Run only linting
#   ./scripts/run_ci.sh format       # Run only formatting
#   ./scripts/run_ci.sh typecheck    # Run only type checking
#   ./scripts/run_ci.sh clean        # Clean artifacts

set -euo pipefail

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PYTHON_CMD="python3"
PIP_CMD="pip3"

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

# Check if dependencies are installed
check_dependencies() {
    log_info "Checking CI dependencies..."
    
    if ! command -v $PYTHON_CMD &> /dev/null; then
        log_error "Python3 is not installed"
        exit 1
    fi
    
    if ! command -v $PIP_CMD &> /dev/null; then
        log_error "pip3 is not installed"
        exit 1
    fi
    
    log_success "CI dependencies check passed"
}

# Install CI dependencies
install_ci_deps() {
    log_info "Installing CI dependencies..."
    
    cd "$PROJECT_DIR"
    
    # Install in user space to avoid PEP 668 issues
    if $PIP_CMD install --user --break-system-packages pytest pytest-cov black flake8 mypy types-requests 2>/dev/null; then
        log_success "CI dependencies installed"
    else
        log_error "Failed to install CI dependencies"
        exit 1
    fi
}

# Run tests
run_tests() {
    log_info "Running CI tests..."
    
    cd "$PROJECT_DIR"
    
    # Ensure pytest can find the module
    export PYTHONPATH="${PROJECT_DIR}:/usr/local/lib/python3.12/site-packages:$PYTHONPATH"
    
    # Run unit tests
    $PYTHON_CMD -m pytest tests/test_client.py tests/test_examples.py -v --tb=short
    
    log_success "CI tests passed"
}

# Run linting
run_lint() {
    log_info "Running linters..."
    
    cd "$PROJECT_DIR"
    
    # Ensure modules can be found
    export PYTHONPATH="${PYTHONPATH:-}:${PROJECT_DIR}:/usr/local/lib/python3.12/site-packages"
    
    # Check code formatting
    log_info "Checking code formatting with black..."
    $PYTHON_CMD -m black --check gridcraft_mcp examples scripts 2>/dev/null || {
        log_error "Code formatting check failed. Run: black gridcraft_mcp examples scripts"
        exit 1
    }
    
    # Run flake8
    log_info "Running flake8 linter..."
    $PYTHON_CMD -m flake8 gridcraft_mcp examples scripts --max-line-length=88 --extend-ignore=E203,W503 || {
        log_error "Flake8 linting failed"
        exit 1
    }
    
    log_success "Linting passed"
}

# Run type checking
run_typecheck() {
    log_info "Running type checking..."
    
    cd "$PROJECT_DIR"
    
    # Ensure mypy can find the module
    export MYPYPATH="${PROJECT_DIR}:/usr/local/lib/python3.12/site-packages"
    export PYTHONPATH="${PROJECT_DIR}:/usr/local/lib/python3.12/site-packages:$PYTHONPATH"
    
    $PYTHON_CMD -m mypy gridcraft_mcp --disallow-untyped-defs --warn-return-any || {
        log_error "Type checking failed"
        exit 1
    }
    
    log_success "Type checking passed"
}

# Run formatting
run_format() {
    log_info "Running code formatting..."
    
    cd "$PROJECT_DIR"
    
    $PYTHON_CMD -m black gridcraft_mcp examples scripts
    $PYTHON_CMD -m isort gridcraft_mcp examples scripts
    
    log_success "Code formatting completed"
}

# Clean artifacts
clean_ci() {
    log_info "Cleaning CI artifacts..."
    
    cd "$PROJECT_DIR"
    
    # Remove coverage reports
    if [ -d "htmlcov" ]; then
        rm -rf htmlcov
    fi
    
    # Remove pytest cache
    if [ -d ".pytest_cache" ]; then
        rm -rf .pytest_cache
    fi
    
    # Remove coverage files
    find . -name ".coverage*" -delete 2>/dev/null || true
    find . -name "*.pyc" -delete 2>/dev/null || true
    find . -name "__pycache__" -type d -exec rm -rf {} + 2>/dev/null || true
    
    log_success "CI artifacts cleaned"
}

# Display CI summary
show_ci_summary() {
    echo ""
    echo "╔════════════════════════════════════════════════════════════╗"
    echo "║           GridCraft MCP Client - CI Summary              ║"
    echo "╚════════════════════════════════════════════════════════════╝"
    echo ""
    
    echo "✅ CI Checks:"
    echo "   • Unit tests (30+ tests)"
    echo "   • Code formatting (black)"
    echo "   • Linting (flake8)"
    echo "   • Type checking (mypy)"
    echo "   • Example validation"
    echo ""
    
    echo "📝 CI Commands:"
    echo "   ./scripts/run_ci.sh test         - Run tests only"
    echo "   ./scripts/run_ci.sh lint         - Run linting only"
    echo "   ./scripts/run_ci.sh typecheck    - Run type checking only"
    echo "   ./scripts/run_ci.sh format       - Format code"
    echo "   ./scripts/run_ci.sh all          - Run all CI checks (default)"
    echo ""
}

# Main execution
main() {
    echo ""
    echo "╔════════════════════════════════════════════════════════════╗"
    echo "║      GridCraft MCP Client - CI/CD Runner Script           ║"
    echo "╚════════════════════════════════════════════════════════════╝"
    echo ""
    
    # Check command line arguments
    case "${1:-all}" in
        "test")
            check_dependencies
            install_ci_deps
            run_tests
            ;;
        
        "lint")
            check_dependencies
            install_ci_deps
            run_lint
            ;;
        
        "typecheck")
            check_dependencies
            install_ci_deps
            run_typecheck
            ;;
        
        "format")
            check_dependencies
            install_ci_deps
            run_format
            ;;
        
        "clean")
            clean_ci
            ;;
        
        "all"|"")
            check_dependencies
            install_ci_deps
            run_tests
            run_lint
            run_typecheck
            ;;
        
        *)
            log_error "Unknown command: $1"
            echo ""
            show_ci_summary
            exit 1
            ;;
    esac
    
    # Always show summary
    show_ci_summary
    
    exit 0
}

# Execute main function
main "$@"
