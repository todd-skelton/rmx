# Build the rmx benchmark: release server binary + size-optimized WASM client.
# Usage: pwsh ./build.ps1 [-SkipServer]
param([switch]$SkipServer)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

& ./build-client.ps1 -Crate bench-client -Out examples/bench/public/pkg

if (-not $SkipServer) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    cargo build -p bench-server --release
    if ($LASTEXITCODE) { throw "server build failed" }
    "server: $([math]::Round($sw.Elapsed.TotalSeconds,1)) s"
}
