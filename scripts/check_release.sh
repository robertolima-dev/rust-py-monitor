#!/usr/bin/env bash
# Pre-release validation script.
# Run before publishing to TestPyPI or PyPI.
set -e

echo "=== 1. Rust tests ==="
cargo test

echo ""
echo "=== 2. Release build ==="
maturin build --release

echo ""
echo "=== 3. Wheel check ==="
python -m twine check target/wheels/*.whl

echo ""
echo "=== 4. Python tests ==="
python -m pytest tests/ -v

echo ""
echo "=== 5. Wheel size ==="
ls -lh target/wheels/

echo ""
echo "All checks passed. Ready to publish."
echo ""
echo "TestPyPI:  maturin publish --repository-url https://test.pypi.org/legacy/"
echo "PyPI:      maturin publish"
