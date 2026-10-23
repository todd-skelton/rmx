# Runs the full rmx vs Remix 3 comparison and writes results/*.json.
#   pwsh ./run.ps1 [-Duration 15] [-Connections 50] [-SkipBrowser] [-SkipServer] [-Suffix -2026-10-22]
# Prereqs: rmx built (../scripts/build-bench.ps1) and `npm install` in remix-app.
param(
    [int]$Duration = 15,
    [int]$Connections = 50,
    [int]$OhaThreads = 4,
    [switch]$SkipBrowser,
    [switch]$SkipServer,
    # Appended to the result file names, so a new run does not replace an earlier one.
    [string]$Suffix = ""
)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
New-Item -ItemType Directory -Force results | Out-Null
$oha = Join-Path $PSScriptRoot "tools/oha.exe"
$rmxDir = Resolve-Path ".."
$remixDir = Resolve-Path "remix-app"

function Stop-Port([int]$port) {
    Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue |
        ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 500
}

# Start a server, return its process and the time until the first 200 response.
function Start-Server([string]$name, [hashtable]$envVars) {
    foreach ($k in $envVars.Keys) { Set-Item "env:$k" $envVars[$k] }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    if ($name -like "remix*") {
        $env:NODE_ENV = "production"; $env:PORT = "44100"
        $script = if ($name -eq "remix-cluster") { "cluster.ts" } else { "server.ts" }
        $p = Start-Process node -ArgumentList "--import", "remix/node-tsx", $script -WorkingDirectory $remixDir -WindowStyle Hidden -PassThru
        $port = 44100
    } else {
        $env:PORT = "44101"
        $p = Start-Process (Join-Path $rmxDir "target/release/bench-server.exe") -WorkingDirectory $rmxDir -WindowStyle Hidden -PassThru
        $port = 44101
    }
    while ($true) {
        try {
            $r = Invoke-WebRequest "http://127.0.0.1:$port/" -TimeoutSec 2
            if ($r.StatusCode -eq 200) { break }
        } catch { Start-Sleep -Milliseconds 20 }
        if ($sw.Elapsed.TotalSeconds -gt 30) { throw "$name did not start" }
    }
    foreach ($k in $envVars.Keys) { Remove-Item "env:$k" -ErrorAction SilentlyContinue }
    [pscustomobject]@{ Process = $p; Port = $port; StartupMs = [math]::Round($sw.Elapsed.TotalMilliseconds) }
}

# Working set of a process plus its direct children (Node cluster workers).
function Get-TreeMemoryMB([int]$id) {
    $procs = @(Get-CimInstance Win32_Process -Filter "ProcessId=$id OR ParentProcessId=$id")
    if ($procs.Count -eq 0) { return $null }
    [math]::Round(($procs | Measure-Object WorkingSetSize -Sum).Sum / 1MB, 1)
}

function Stop-Tree([int]$id) {
    Get-CimInstance Win32_Process -Filter "ParentProcessId=$id" |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
}

function Invoke-Load([int]$port, [string]$path, [int]$seconds) {
    $json = & $oha -z "${seconds}s" -c $Connections --worker-threads $OhaThreads --no-tui --output-format json "http://127.0.0.1:$port$path"
    $j = $json | ConvertFrom-Json
    $codes = ($j.statusCodeDistribution.PSObject.Properties | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join " "
    [pscustomobject]@{
        Rps      = [math]::Round($j.summary.requestsPerSec)
        P50Ms    = [math]::Round($j.latencyPercentiles.p50 * 1000, 2)
        P99Ms    = [math]::Round($j.latencyPercentiles.p99 * 1000, 2)
        Bytes    = $j.summary.sizePerRequest
        Success  = $j.summary.successRate
        Codes    = $codes
    }
}

Stop-Port 44100
Stop-Port 44101

if (-not $SkipBrowser) {
    $remix = Start-Server "remix" @{}
    $rmx = Start-Server "rmx" @{}
    # Remix compiles client modules on first request; load once so the timed runs are warm.
    Invoke-WebRequest "http://127.0.0.1:44100/client" | Out-Null
    node browser-bench.mjs --iterations 15 --loads 10 --out "results/browser$Suffix.json"
    Stop-Process -Id $remix.Process.Id, $rmx.Process.Id -Force
    Stop-Port 44100; Stop-Port 44101
}

if (-not $SkipServer) {
    $cores = (Get-CimInstance Win32_Processor).NumberOfLogicalProcessors
    $configs = @(
        @{ Name = "remix (node, 1 process)"; Server = "remix"; Env = @{} },
        @{ Name = "remix (node cluster, $cores workers)"; Server = "remix-cluster"; Env = @{} },
        @{ Name = "rmx (1 thread)"; Server = "rmx"; Env = @{ RMX_THREADS = "1" } },
        @{ Name = "rmx ($cores threads)"; Server = "rmx"; Env = @{} }
    )
    $paths = @("/", "/ssr/100", "/ssr/1000")
    $rows = @()
    foreach ($c in $configs) {
        $s = Start-Server $c.Server $c.Env
        $idleMB = Get-TreeMemoryMB $s.Process.Id
        foreach ($path in $paths) {
            Invoke-Load $s.Port $path 3 | Out-Null  # warm-up (JIT, caches)
            $r = Invoke-Load $s.Port $path $Duration
            $row = [pscustomobject]@{
                Config = $c.Name; Path = $path; Rps = $r.Rps; P50Ms = $r.P50Ms; P99Ms = $r.P99Ms
                Bytes = $r.Bytes; Success = $r.Success; Codes = $r.Codes
                StartupMs = $s.StartupMs; IdleMB = $idleMB; LoadedMB = Get-TreeMemoryMB $s.Process.Id
            }
            $rows += $row
            $row | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
        }
        Stop-Tree $s.Process.Id
        Stop-Port $s.Port
    }
    $meta = [pscustomobject]@{
        date = (Get-Date).ToString("o"); duration_s = $Duration; connections = $Connections
        oha_threads = $OhaThreads; cpu = (Get-CimInstance Win32_Processor).Name; logical_cores = $cores
        node = (node --version); rows = $rows
    }
    $meta | ConvertTo-Json -Depth 5 | Set-Content "results/server$Suffix.json" -Encoding utf8
    $rows | Format-Table Config, Path, Rps, P50Ms, P99Ms, Bytes, StartupMs, IdleMB, LoadedMB -AutoSize | Out-String -Width 200
}
