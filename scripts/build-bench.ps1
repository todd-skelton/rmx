# Build the rmx benchmark: release server binary + size-optimized WASM client.
# Usage: pwsh ./build.ps1 [-SkipServer]
param([switch]$SkipServer)
$ErrorActionPreference = "Stop"
# Paths are relative to the repository root.
Set-Location (Split-Path $PSScriptRoot -Parent)

& ./scripts/build-client.ps1 -Crate bench-client -Out bench/rmx-app/public/pkg

if (-not $SkipServer) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    cargo build -p bench-server --release
    if ($LASTEXITCODE) { throw "server build failed" }
    "server: $([math]::Round($sw.Elapsed.TotalSeconds,1)) s"
}
