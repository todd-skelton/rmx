# Linux/Docker variant of the server benchmark (run.ps1 covers Windows + the browser bench).
#   pwsh ./run-docker.ps1 [-Duration 15] [-Connections 50] [-Build] [-Only rmx] [-RmxImage rmxbench-rmx]
# Each server runs alone in a container pinned to the same CPUs and memory cap; oha runs in
# its own container on separate CPUs and reaches the server over a Docker network, so no
# Windows port forwarding or bind mounts are on the measured path.
param(
    [int]$Duration = 15,
    [int]$Warmup = 3,
    [int]$Connections = 50,
    [int]$OhaThreads = 4,
    [string]$OneCpu = "2",
    [string]$ManyCpus = "0-7",
    [string]$OhaCpus = "8-15",
    [string]$Only = "",  # regex on config name, e.g. "1 (process|thread)"
    [string]$Out = "docker-server.json",
    [string]$Memory = "8g",
    [string]$RmxImage = "rmxbench-rmx",  # another rmx build, e.g. one tagged from an older commit
    [switch]$Build
)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$results = Join-Path $PSScriptRoot "../results"
New-Item -ItemType Directory -Force $results | Out-Null
$net = "rmxbench"

if ($Build) {
    docker build -t rmxbench-oha -f oha.Dockerfile . ; if ($LASTEXITCODE) { throw "oha build failed" }
    docker build -t rmxbench-remix -f remix.Dockerfile ../remix-app ; if ($LASTEXITCODE) { throw "remix build failed" }
    docker build -t rmxbench-rmx -f rmx.Dockerfile ../.. ; if ($LASTEXITCODE) { throw "rmx build failed" }
}

docker network inspect $net *> $null
if ($LASTEXITCODE) { docker network create $net | Out-Null }

function Remove-Server { docker rm -f rb-server *> $null }

function Get-MemMB {
    $raw = (docker stats --no-stream --format "{{.MemUsage}}" rb-server).Split("/")[0].Trim()
    if ($raw -match "^([\d.]+)\s*([KMG]i?B)$") {
        $v = [double]$Matches[1]
        switch -regex ($Matches[2]) { "^K" { $v /= 1024 } "^G" { $v *= 1024 } }
        return [math]::Round($v, 1)
    }
    $raw
}

# Start the server container; return startup time (docker run -> first 200 from inside the network).
function Start-Server($c) {
    Remove-Server
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $runArgs = @("run", "-d", "--name", "rb-server", "--network", $net, "--cpuset-cpus", $c.Cpus, "--memory", $Memory)
    foreach ($k in $c.Env.Keys) { $runArgs += @("-e", "$k=$($c.Env[$k])") }
    $runArgs += $c.Image
    $runArgs += $c.Cmd
    docker @runArgs | Out-Null
    if ($LASTEXITCODE) { throw "failed to start $($c.Name)" }
    while ($true) {
        # Short oha probe from the load-generator image; exits 0 once the port answers.
        $probe = docker run --rm --network $net rmxbench-oha -n 1 --no-tui --output-format json "http://rb-server:$($c.Port)/" 2>$null | ConvertFrom-Json -ErrorAction SilentlyContinue
        if ($probe -and $probe.statusCodeDistribution.'200' -eq 1) { break }
        if ($sw.Elapsed.TotalSeconds -gt 60) { docker logs rb-server; throw "$($c.Name) did not start" }
        Start-Sleep -Milliseconds 100
    }
    [math]::Round($sw.Elapsed.TotalMilliseconds)
}

function Invoke-Load([int]$port, [string]$path, [int]$seconds) {
    $json = docker run --rm --network $net --cpuset-cpus $OhaCpus rmxbench-oha -z "${seconds}s" -c $Connections --worker-threads $OhaThreads --no-tui --output-format json "http://rb-server:$port$path"
    $j = $json | ConvertFrom-Json
    [pscustomobject]@{
        Rps     = [math]::Round($j.summary.requestsPerSec)
        P50Ms   = [math]::Round($j.latencyPercentiles.p50 * 1000, 2)
        P99Ms   = [math]::Round($j.latencyPercentiles.p99 * 1000, 2)
        Bytes   = $j.summary.sizePerRequest
        Success = $j.summary.successRate
        Codes   = ($j.statusCodeDistribution.PSObject.Properties | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join " "
    }
}

$tsx = @("node", "--import", "remix/node-tsx")
$configs = @(
    @{ Name = "remix (node, 1 process)"; Cores = 1; Cpus = $OneCpu; Image = "rmxbench-remix"; Port = 44100; Env = @{}; Cmd = $tsx + "server.ts" },
    @{ Name = "rmx (1 thread)"; Cores = 1; Cpus = $OneCpu; Image = $RmxImage; Port = 44101; Env = @{ RMX_THREADS = "1" }; Cmd = @() },
    @{ Name = "remix (node cluster, 8 workers)"; Cores = 8; Cpus = $ManyCpus; Image = "rmxbench-remix"; Port = 44100; Env = @{ REMIX_WORKERS = "8" }; Cmd = $tsx + "cluster.ts" },
    @{ Name = "rmx (8 threads)"; Cores = 8; Cpus = $ManyCpus; Image = $RmxImage; Port = 44101; Env = @{ RMX_THREADS = "8" }; Cmd = @() }
)
if ($Only) { $configs = @($configs | Where-Object { $_.Name -match $Only }) }
$paths = @("/", "/ssr/100", "/ssr/1000")
$rows = @()
try {
    foreach ($c in $configs) {
        $startup = Start-Server $c
        Start-Sleep -Seconds 1
        $idle = Get-MemMB
        foreach ($path in $paths) {
            Invoke-Load $c.Port $path $Warmup | Out-Null
            $r = Invoke-Load $c.Port $path $Duration
            $row = [pscustomobject]@{
                Config = $c.Name; Cores = $c.Cores; Path = $path; Rps = $r.Rps; P50Ms = $r.P50Ms; P99Ms = $r.P99Ms
                Bytes = $r.Bytes; Success = $r.Success; Codes = $r.Codes
                StartupMs = $startup; IdleMB = $idle; LoadedMB = Get-MemMB
            }
            $rows += $row
            Write-Host ("{0,-34} {1,-10} {2,8} req/s  p50 {3,8} ms  p99 {4,8} ms  {5}" -f $row.Config, $row.Path, $row.Rps, $row.P50Ms, $row.P99Ms, $row.Codes)
        }
        Remove-Server
    }
} finally {
    Remove-Server
}

$meta = [pscustomobject]@{
    date = (Get-Date).ToString("o"); duration_s = $Duration; warmup_s = $Warmup; connections = $Connections
    oha_threads = $OhaThreads; oha_cpus = $OhaCpus; one_cpu = $OneCpu; many_cpus = $ManyCpus; memory_limit = $Memory
    docker = (docker version --format "{{.Server.Version}} {{.Server.KernelVersion}}")
    node = (docker run --rm rmxbench-remix node --version)
    rows = $rows
}
$meta | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $results $Out) -Encoding utf8
$rows | Format-Table Config, Path, Rps, P50Ms, P99Ms, Bytes, StartupMs, IdleMB, LoadedMB -AutoSize | Out-String -Width 200
