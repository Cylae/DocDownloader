# DocDownloader Unified Verification Script (Windows PowerShell)
$ErrorActionPreference = "Stop"

Write-Host "=== Step 1: Formatting Check ===" -ForegroundColor Cyan
cargo fmt --check
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo fmt check failed"
    exit 1
}

Write-Host "=== Step 2: Clippy Linting (Strict -D warnings) ===" -ForegroundColor Cyan
cargo clippy --all-targets --all-features -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo clippy check failed"
    exit 1
}

Write-Host "=== Step 3: Test Suite (Unit, Integration, Security, Property Tests) ===" -ForegroundColor Cyan
cargo test --all-targets --all-features
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo test failed"
    exit 1
}

Write-Host "=== Step 4: Synthetic Benchmarks Execution ===" -ForegroundColor Cyan
cargo bench --bench synthetic_bench
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo bench failed"
    exit 1
}

Write-Host "=== Step 5: Release Build Smoke Test ===" -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build --release failed"
    exit 1
}

Write-Host "`nAll verification gates passed successfully!" -ForegroundColor Green
