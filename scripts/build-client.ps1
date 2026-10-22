# Build a browser crate to WebAssembly: cargo (wasm-release profile) + wasm-bindgen + wasm-opt.
# Usage: pwsh ./scripts/build-client.ps1 -Crate bench-client -Out bench/rmx-app/public/pkg
param(
    [Parameter(Mandatory)] [string]$Crate,
    [Parameter(Mandatory)] [string]$Out,
    [switch]$Dev
)
$ErrorActionPreference = "Stop"
# Paths are relative to the repository root.
Set-Location (Split-Path $PSScriptRoot -Parent)

$tools = Join-Path $env:LOCALAPPDATA "cargo-leptos"
$wasmBindgen = Get-ChildItem $tools -Recurse -Filter wasm-bindgen.exe | Where-Object FullName -match "0\.2\.129" | Select-Object -First 1
$wasmOpt = Get-ChildItem $tools -Recurse -Filter wasm-opt.exe | Select-Object -First 1
if (-not $wasmBindgen -or -not $wasmOpt) { throw "wasm-bindgen 0.2.129 / wasm-opt not found under $tools" }

$profile = if ($Dev) { "dev" } else { "wasm-release" }
$dir = if ($Dev) { "debug" } else { "wasm-release" }
$name = $Crate -replace "-", "_"
$sw = [Diagnostics.Stopwatch]::StartNew()
cargo build -p $Crate --lib --target wasm32-unknown-unknown --profile $profile
if ($LASTEXITCODE) { throw "$Crate build failed" }
& $wasmBindgen.FullName --target web --no-typescript --out-dir $Out "target/wasm32-unknown-unknown/$dir/$name.wasm"
if ($LASTEXITCODE) { throw "wasm-bindgen failed" }
if (-not $Dev) {
    & $wasmOpt.FullName -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types -o "$Out/${name}_bg.wasm" "$Out/${name}_bg.wasm"
    if ($LASTEXITCODE) { throw "wasm-opt failed" }
}
$size = (Get-Item "$Out/${name}_bg.wasm").Length
"${Crate}: $([math]::Round($sw.Elapsed.TotalSeconds,1)) s, $([math]::Round($size/1KB,1)) KiB wasm"
