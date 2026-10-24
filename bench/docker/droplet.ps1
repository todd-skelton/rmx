# Run the Docker server bench (and the router profile) on a fresh DigitalOcean droplet with
# dedicated CPUs, copy the results back, and destroy the droplet. Needs doctl (authenticated),
# ssh, scp and tar on this machine, and a DigitalOcean SSH key whose private half is in ~/.ssh.
#
#   pwsh bench/docker/droplet.ps1 -Suffix 2026-10-24-droplet [-AbRefs b70fd12,bbdaed2] [-Rounds 2]
#
# The working tree's HEAD is benchmarked against Remix (run-docker.ps1 -Build); each -AbRefs
# commit is built as rmxbench-rmx-<ref> and run rmx-only, alternating with HEAD, -Rounds times.
# The browser bundle (bench/rmx-app/public/pkg, not in git) is copied from this machine.
# Results land in bench/results/droplet-<Suffix>/.
param(
    [Parameter(Mandatory)] [string]$Suffix,
    [string[]]$AbRefs = @(),
    [int]$Rounds = 2,
    [string]$Size = "c-16",
    [string]$Region = "nyc3",
    [string]$SshKeyId = "",
    [string]$Identity = (Join-Path $HOME ".ssh/id_rsa")
)
$ErrorActionPreference = "Stop"
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
Set-Location $repo
$work = Join-Path $repo "target/droplet-$Suffix"
New-Item -ItemType Directory -Force $work | Out-Null
$results = Join-Path $repo "bench/results/droplet-$Suffix"
New-Item -ItemType Directory -Force $results | Out-Null

if (-not $SshKeyId) {
    $fingerprint = ((ssh-keygen -l -E md5 -f "$Identity.pub") -split " ")[1] -replace "^MD5:", ""
    $SshKeyId = (doctl compute ssh-key list --format ID,FingerPrint --no-header | Where-Object { $_ -match [regex]::Escape($fingerprint) } | ForEach-Object { ($_ -split "\s+")[0] } | Select-Object -First 1)
    if (-not $SshKeyId) { throw "no DigitalOcean SSH key matches $Identity.pub" }
}

# Sources: HEAD and each A/B ref as tarballs, plus the browser bundle.
git archive --format=tar -o (Join-Path $work "head.tar") HEAD
foreach ($ref in $AbRefs) { git archive --format=tar -o (Join-Path $work "ref-$ref.tar") $ref }
tar -cf (Join-Path $work "pkg.tar") -C bench/rmx-app/public pkg
Copy-Item (Join-Path $PSScriptRoot "droplet-run.sh") $work

$ssh = @("-i", $Identity, "-o", "StrictHostKeyChecking=accept-new", "-o", "UserKnownHostsFile=$work/known_hosts", "-o", "ConnectTimeout=10")
$id = $null
try {
    $created = doctl compute droplet create "rmx-bench-$($Suffix.ToLower())" --size $Size --region $Region --image ubuntu-24-04-x64 --ssh-keys $SshKeyId --tag-name rmx-bench --wait --format ID,PublicIPv4 --no-header
    if ($LASTEXITCODE) { throw "droplet create failed" }
    $id, $ip = ($created -split "\s+") | Where-Object { $_ }
    Write-Host "droplet $id at $ip"
    $deadline = (Get-Date).AddMinutes(5)
    while ($true) {
        ssh @ssh "root@$ip" true 2>$null
        if ($LASTEXITCODE -eq 0) { break }
        if ((Get-Date) -gt $deadline) { throw "ssh to $ip did not come up" }
        Start-Sleep -Seconds 5
    }
    $uploads = @(Get-ChildItem $work -Filter "*.tar" | Where-Object Name -ne "out.tar" | ForEach-Object FullName) + (Join-Path $work "droplet-run.sh")
    scp @ssh @uploads "root@${ip}:/root/"
    if ($LASTEXITCODE) { throw "upload failed" }
    # The remote run writes progress to stderr too; keep it in the log instead of stopping.
    $ErrorActionPreference = "Continue"
    ssh @ssh "root@$ip" "export PATH=`$PATH:/snap/bin SUFFIX='$Suffix' AB_REFS='$($AbRefs -join " ")' ROUNDS=$Rounds; bash /root/droplet-run.sh" 2>&1 | ForEach-Object { "$_" } | Tee-Object -FilePath (Join-Path $results "run.log")
    $code = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    scp @ssh "root@${ip}:/root/out.tar" (Join-Path $work "out.tar")
    if ($LASTEXITCODE -eq 0) { tar -xf (Join-Path $work "out.tar") -C $results }
    if ($code) { throw "the run on the droplet failed ($code)" }
} finally {
    if ($id) {
        Write-Host "destroying droplet $id"
        doctl compute droplet delete $id --force
    }
}
