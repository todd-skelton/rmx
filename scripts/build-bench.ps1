# Build the rsmix benchmark: release server binary + size-optimized WASM client.
# Usage: pwsh ./scripts/build-bench.ps1 [-SkipServer] [-Nightly nightly]
# -Nightly: build the client with a nightly toolchain and immediate-abort panics (see build-client.ps1).
param([switch]$SkipServer, [string]$Nightly)
$ErrorActionPreference = "Stop"
# Paths are relative to the repository root.
Set-Location (Split-Path $PSScriptRoot -Parent)

& ./scripts/build-client.ps1 -Crate bench-client -Out bench/rsmix-app/public/pkg -Nightly $Nightly

if (-not $SkipServer) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    cargo build -p bench-server --release
    if ($LASTEXITCODE) { throw "server build failed" }
    "server: $([math]::Round($sw.Elapsed.TotalSeconds,1)) s"
}
