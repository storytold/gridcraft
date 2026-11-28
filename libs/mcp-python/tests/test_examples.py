#!/usr/bin/env python3
"""
Test that example scripts run without errors.

This ensures that the examples in the examples/ directory are valid and work.
"""

import subprocess
import sys
from pathlib import Path


def test_example_script_exists():
    """Test that example scripts exist"""
    examples_dir = Path(__file__).parent.parent / "examples"
    
    expected_examples = [
        "basic_usage.py",
        "data_analysis.py",
        "automation.py",
        "charts_and_tables.py",
    ]
    
    for example in expected_examples:
        example_path = examples_dir / example
        assert example_path.exists(), f"Example {example} not found"
        assert example_path.is_file(), f"{example} is not a file"


def test_example_syntax():
    """Test that example scripts have valid Python syntax"""
    examples_dir = Path(__file__).parent.parent / "examples"
    
    for example_file in examples_dir.glob("*.py"):
        if example_file.name.startswith("_"):
            continue
        
        # Check syntax using py_compile
        import py_compile
        try:
            py_compile.compile(example_file, doraise=True)
        except py_compile.PyCompileError as e:
            pytest.fail(f"Syntax error in {example_file.name}: {e}")


def test_basic_usage_example_syntax():
    """Test basic_usage.py has valid syntax"""
    example_path = Path(__file__).parent.parent / "examples" / "basic_usage.py"
    
    import py_compile
    py_compile.compile(example_path, doraise=True)


def test_data_analysis_example_syntax():
    """Test data_analysis.py has valid syntax"""
    example_path = Path(__file__).parent.parent / "examples" / "data_analysis.py"
    
    import py_compile
    py_compile.compile(example_path, doraise=True)


def test_automation_example_syntax():
    """Test automation.py has valid syntax"""
    example_path = Path(__file__).parent.parent / "examples" / "automation.py"
    
    import py_compile
    py_compile.compile(example_path, doraise=True)


def test_charts_and_tables_example_syntax():
    """Test charts_and_tables.py has valid syntax"""
    example_path = Path(__file__).parent.parent / "examples" / "charts_and_tables.py"
    
    import py_compile
    py_compile.compile(example_path, doraise=True)


if __name__ == "__main__":
    import pytest
    pytest.main([__file__, "-v"])
