#requires -Version 7.0
# Fetch and verify the isolated C reference; never modify the sibling checkout.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$manifest = Get-Content -LiteralPath "$root/docs/reference-manifest.json" -Raw | ConvertFrom-Json
$reference = $manifest.source
$target = Join-Path $root '.reference/cbirds'
function Invoke-ReferenceGit {
    & git @args
    if ($LASTEXITCODE -ne 0) { throw "git failed ($LASTEXITCODE): $args" }
}
if (-not (Test-Path -LiteralPath "$target/.git")) {
    $sibling = Join-Path (Split-Path $root -Parent) 'cbirds'
    $source = if (Test-Path -LiteralPath "$sibling/.git") { $sibling } else { $reference.repository }
    Invoke-ReferenceGit -c core.autocrlf=false clone --no-hardlinks --no-checkout $source $target
    Invoke-ReferenceGit -C $target config core.autocrlf false
    Invoke-ReferenceGit -C $target checkout --quiet --detach $reference.commit
}
$head = Invoke-ReferenceGit -C $target rev-parse HEAD
if ($head -ne $reference.commit) { throw "Reference is at $head, expected $($reference.commit). Existing checkouts are not changed." }
$changes = Invoke-ReferenceGit -C $target status --porcelain --untracked-files=no
if ($changes) { throw 'Reference has tracked changes; existing checkouts are not changed.' }
foreach ($file in $reference.sha256_by_path.PSObject.Properties) {
    $actual = (Get-FileHash -LiteralPath (Join-Path $target $file.Name) -Algorithm SHA256).Hash
    if ($actual -ne $file.Value) { throw "Reference hash mismatch: $($file.Name)" }
}
Write-Host "Reference verified: $target at $head ($($reference.tracked_file_count) files)"
