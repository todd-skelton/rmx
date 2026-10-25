# Publish every library crate to crates.io in dependency order, like Remix's release scripts.
# Skips versions that are already published, so it can be rerun after a failure, and waits out
# crates.io's rate limit for new crates (a 429 says when to try again).
# Usage: pwsh ./scripts/publish.ps1 [-DryRun]
param([switch]$DryRun)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)
$agent = "rsmix-publish (https://github.com/todd-skelton/rsmix)"

$meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
$crates = @{}
$meta.packages | Where-Object { $null -eq $_.publish } | ForEach-Object { $crates[$_.name] = $_ }
$deps = @{}
foreach ($c in $crates.Values) {
    $deps[$c.name] = @($c.dependencies | Where-Object { $_.kind -ne "dev" -and $crates.ContainsKey($_.name) } |
        ForEach-Object name | Select-Object -Unique)
}
$order = [System.Collections.Generic.List[string]]::new()
$placed = @{}
while ($order.Count -lt $crates.Count) {
    $ready = $deps.Keys | Where-Object { -not $placed[$_] -and @($deps[$_] | Where-Object { -not $placed[$_] }).Count -eq 0 } | Sort-Object
    if (-not $ready) { throw "dependency cycle among unpublished crates" }
    foreach ($r in $ready) { $order.Add($r); $placed[$r] = $true }
}

function Test-Published([string]$name, [string]$version) {
    $code = curl.exe -s -o NUL -w "%{http_code}" -A $agent "https://crates.io/api/v1/crates/$name/$version"
    return $code -eq "200"
}

& (Join-Path $PSScriptRoot "sync-cli-template.ps1")
try {
    $i = 0
    foreach ($name in $order) {
        $i++
        $version = $crates[$name].version
        if (Test-Published $name $version) { Write-Host "[$i/$($order.Count)] $name $version is already published"; continue }
        while ($true) {
            Write-Host "[$i/$($order.Count)] $(Get-Date -Format s) publishing $name $version"
            $args = @("publish", "-p", $name)
            if ($DryRun) { $args += "--dry-run" }
            $out = & cargo @args 2>&1 | ForEach-Object { "$_" }
            $out | Write-Host
            if ($LASTEXITCODE -eq 0) { break }
            $text = $out -join "`n"
            if ($text -match "already exists") { Write-Host "$name $version was published meanwhile"; break }
            if ($text -match "try again after ([A-Za-z]{3}, \d{1,2} [A-Za-z]{3} \d{4} \d{2}:\d{2}:\d{2} GMT)") {
                $until = [DateTime]::Parse($Matches[1]).ToUniversalTime().AddSeconds(30)
                $wait = [Math]::Max(30, ($until - [DateTime]::UtcNow).TotalSeconds)
                Write-Host "rate limited; waiting $([int]$wait) s until $($until.ToString('u'))"
                Start-Sleep -Seconds $wait
                continue
            }
            if ($text -match "429|Too Many Requests") { Write-Host "rate limited; waiting 600 s"; Start-Sleep -Seconds 600; continue }
            throw "cargo publish -p $name failed"
        }
    }
    Write-Host "all $($order.Count) crates published"
} finally {
    & (Join-Path $PSScriptRoot "sync-cli-template.ps1") -Clean
}
