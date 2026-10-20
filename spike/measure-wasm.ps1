# Reports raw, gzip and brotli sizes of the browser bundle cargo-leptos wrote to target/site/pkg.
# Usage: pwsh ./measure-wasm.ps1 [-Label "islands release"]
param([string]$Label = "")

$pkg = Join-Path $PSScriptRoot "target/site/pkg"
if (-not (Test-Path -LiteralPath $pkg)) { throw "No bundle at $pkg; run cargo leptos build first." }

function Get-CompressedSize([byte[]]$bytes, [string]$kind) {
    $ms = [IO.MemoryStream]::new()
    $level = [IO.Compression.CompressionLevel]::SmallestSize
    $z = if ($kind -eq "gzip") { [IO.Compression.GZipStream]::new($ms, $level) }
         else { [IO.Compression.BrotliStream]::new($ms, $level) }
    $z.Write($bytes, 0, $bytes.Length)
    $z.Dispose()
    $ms.ToArray().Length
}

$rows = Get-ChildItem -LiteralPath $pkg -File | Where-Object { $_.Extension -in ".wasm", ".js" } | ForEach-Object {
    $b = [IO.File]::ReadAllBytes($_.FullName)
    [pscustomobject]@{
        Label  = $Label
        File   = $_.Name
        RawKB  = [math]::Round($b.Length / 1KB, 1)
        GzipKB = [math]::Round((Get-CompressedSize $b "gzip") / 1KB, 1)
        BrKB   = [math]::Round((Get-CompressedSize $b "brotli") / 1KB, 1)
    }
}
$rows | Format-Table -AutoSize | Out-String -Width 200
