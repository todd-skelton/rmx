# Run a crate's browser tests (tests/browser_*.rs, wasm-bindgen-test) in headless Chromium.
# Usage: pwsh ./test-browser.ps1 [-Crate rmx-component] [-Test browser_errors]
# Env: CHROMEDRIVER (chromedriver.exe), CHROMIUM_PATH (chrome.exe matching the driver).
param([string]$Crate = "rmx-component", [string]$Test)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$tools = Join-Path $env:LOCALAPPDATA "cargo-leptos"
$runner = Get-ChildItem $tools -Recurse -Filter wasm-bindgen-test-runner.exe | Where-Object FullName -match "0\.2\.129" | Select-Object -First 1
if (-not $runner) { throw "wasm-bindgen-test-runner 0.2.129 not found under $tools" }
if (-not $env:CHROMEDRIVER) { $env:CHROMEDRIVER = Join-Path $env:LOCALAPPDATA "chromedriver-148/chromedriver-win64/chromedriver.exe" }
$chrome = if ($env:CHROMIUM_PATH) { $env:CHROMIUM_PATH } else { Join-Path $env:LOCALAPPDATA "ms-playwright/chromium-1223/chrome-win64/chrome.exe" }
if (-not (Test-Path $env:CHROMEDRIVER)) { throw "chromedriver not found: $env:CHROMEDRIVER" }

# wasm-bindgen-test-runner reads webdriver.json from the working directory (the crate).
$crateDir = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).packages | Where-Object name -eq $Crate | ForEach-Object { Split-Path $_.manifest_path }
$caps = @{ "goog:chromeOptions" = @{ binary = $chrome; args = @("--headless=new", "--no-sandbox") } } | ConvertTo-Json -Depth 4
Set-Content -Path (Join-Path $crateDir "webdriver.json") -Value $caps

$env:CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = $runner.FullName
$names = if ($Test) { @($Test) } else { Get-ChildItem (Join-Path $crateDir "tests") -Filter "browser_*.rs" | ForEach-Object BaseName }
$testArgs = $names | ForEach-Object { "--test"; $_ }
try {
    cargo test -p $Crate --target wasm32-unknown-unknown @testArgs
    $code = $LASTEXITCODE
} finally {
    Remove-Item (Join-Path $crateDir "webdriver.json") -ErrorAction SilentlyContinue
}
exit $code
