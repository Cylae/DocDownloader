#!/usr/bin/env bash
# DocDownloader Unified Verification Script (Unix / Linux / macOS)
set -euo pipefail

echo "=== Step 1: Formatting Check ==="
cargo fmt --check

echo "=== Step 2: Clippy Linting (Strict -D warnings) ==="
cargo clippy --all-targets --all-features -- -D warnings

echo "=== Step 3: Test Suite (Unit, Integration, Security, Property Tests) ==="
cargo test --all-targets --all-features

echo "=== Step 4: Synthetic Benchmarks Execution ==="
cargo bench --bench synthetic_bench

echo "=== Step 5: Release Build Smoke Test ==="
cargo build --release

echo ""
echo "All verification gates passed successfully!"
