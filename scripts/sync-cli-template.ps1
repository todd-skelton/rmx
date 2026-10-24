# Copy the starter app into packages/cli/template so the published rsmix-cli crate can embed it,
# like Remix's scripts/sync-cli-template.ts (its cli package runs that on prepack and --clean on
# postpack). In a checkout, packages/cli/build.rs reads the repository's template/ directly, so
# this is only needed before `cargo package` or `cargo publish`.
# Usage: pwsh ./scripts/sync-cli-template.ps1 [-Clean]
param([switch]$Clean)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$source = Join-Path $root "template"
$target = Join-Path $root "packages\cli\template"

# Build output and local files that never belong in a new app. Cargo.toml is left out too:
# `rsmix new` writes the app manifest itself, and cargo package drops any directory that holds a
# Cargo.toml as a nested package.
$excluded = @("target", "pkg", "node_modules", ".git", ".gitignore", ".tmp", "tmp", "Cargo.lock", "Cargo.toml")

function Test-Copyable([string]$name) {
    -not ($excluded -contains $name) -and -not $name.EndsWith(".log") -and
        -not ($name -eq ".env" -or $name -eq ".env.local" -or $name -match '^\.env\..+\.local$')
}

function Copy-Template([string]$from, [string]$to) {
    New-Item -ItemType Directory -Force -Path $to | Out-Null
    foreach ($entry in Get-ChildItem -LiteralPath $from -Force) {
        if (-not (Test-Copyable $entry.Name)) { continue }
        $dest = Join-Path $to $entry.Name
        if ($entry.PSIsContainer) { Copy-Template $entry.FullName $dest }
        else { Copy-Item -LiteralPath $entry.FullName -Destination $dest }
    }
}

function Remove-Target {
    if (-not (Test-Path -LiteralPath $target)) { return }
    $resolved = (Resolve-Path -LiteralPath $target).Path
    if ($resolved -ne [IO.Path]::GetFullPath($target) -or -not $resolved.EndsWith("packages\cli\template")) {
        throw "refusing to remove $resolved"
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}

Remove-Target
if ($Clean) { return }
Copy-Template $source $target
Write-Host "Synced $source -> $target"
