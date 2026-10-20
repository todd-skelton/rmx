# Build the rmx benchmark: release server binary + size-optimized WASM client.
# Usage: pwsh ./build.ps1 [-SkipServer]
param([switch]$SkipServer)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$tools = Join-Path $env:LOCALAPPDATA "cargo-leptos"
$wasmBindgen = Get-ChildItem $tools -Recurse -Filter wasm-bindgen.exe | Where-Object FullName -match "0\.2\.129" | Select-Object -First 1
$wasmOpt = Get-ChildItem $tools -Recurse -Filter wasm-opt.exe | Select-Object -First 1
if (-not $wasmBindgen -or -not $wasmOpt) { throw "wasm-bindgen 0.2.129 / wasm-opt not found under $tools" }

$sw = [Diagnostics.Stopwatch]::StartNew()
cargo build -p bench-client --target wasm32-unknown-unknown --profile wasm-release
if ($LASTEXITCODE) { throw "client build failed" }
$pkg = "examples/bench/public/pkg"
& $wasmBindgen.FullName --target web --no-typescript --out-dir $pkg target/wasm32-unknown-unknown/wasm-release/bench_client.wasm
if ($LASTEXITCODE) { throw "wasm-bindgen failed" }
& $wasmOpt.FullName -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types -o "$pkg/bench_client_bg.wasm" "$pkg/bench_client_bg.wasm"
if ($LASTEXITCODE) { throw "wasm-opt failed" }
"client: $([math]::Round($sw.Elapsed.TotalSeconds,1)) s"

if (-not $SkipServer) {
    $sw.Restart()
    cargo build -p bench-server --release
    if ($LASTEXITCODE) { throw "server build failed" }
    "server: $([math]::Round($sw.Elapsed.TotalSeconds,1)) s"
}
