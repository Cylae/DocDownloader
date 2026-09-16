#!/usr/bin/env bash
# DocDownloader Unified Verification Script (Unix / Linux / macOS)
set -euo pipefail

echo "=== Step 1: Formatting Check ==="
cargo fmt --check

echo "=== Step 2: Clippy Linting (Strict -D warnings) ==="
cargo clippy --all-targets --all-features -- -D warnings

echo "=== Step 3: Test Suite (Unit, Integration, Security, Property Tests) ==="
cargo test --all-targets --all-features

echo "=== Step 4: Security & Supply Chain Audit ==="
if command -v cargo-audit &> /dev/null; then
    cargo audit
else
    echo "cargo-audit not installed; skipping advisory audit"
fi

if command -v cargo-deny &> /dev/null; then
    cargo-deny check
else
    echo "cargo-deny not installed; skipping license and dependency checks"
fi

echo "=== Step 5: Synthetic Benchmarks Execution ==="
cargo bench --bench synthetic_bench

echo "=== Step 6: Release Build Smoke Test ==="
cargo build --release

echo ""
echo "All verification gates passed successfully!"
