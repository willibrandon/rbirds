#requires -Version 7.0
# Native measurements, including Sixel. C-reference budgets remain in perf.sh.
# -BigFlock measures big-flock mode (docs/DEVIATIONS.md D-006): its threads,
# and up to 65536 birds.
[CmdletBinding()]
param(
    [ValidateRange(1, 100)][int]$Samples = 10,
    [ValidateRange(1, 1000000)][int]$Frames = 300,
    [ValidateRange(1, 65536)][int[]]$Birds = @(800, 4096),
    [switch]$BigFlock,
    [ValidateSet('kitty', 'braille', 'sextants', 'blocks', 'sixel')]
    [string[]]$Render = @('kitty', 'braille', 'sextants', 'blocks', 'sixel'),
    [switch]$CompareReference,
    [string]$Distribution = 'Debian'
)
$ErrorActionPreference = 'Stop'
if (-not $BigFlock -and ($Birds | Where-Object { $_ -gt 4096 })) {
    throw 'More than 4096 birds needs -BigFlock'
}
$count_option = if ($BigFlock) { '--big-flock' } else { '--birds' }
if ($CompareReference) {
    & "$PSScriptRoot/unix.ps1" perf -Distribution $Distribution -TaskArguments @("$Samples")
    return
}
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    & cargo build --release --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    $target = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR, $root) } else { Join-Path $root 'target' }
    $exe = Join-Path $target $(if ($IsWindows) { 'release/rbirds.exe' } else { 'release/rbirds' })
    $out = Join-Path $target "perf/$([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))"
    New-Item -ItemType Directory -Path $out -Force | Out-Null
    $rows = [Collections.Generic.List[object]]::new()
    foreach ($mode in $Render) {
        foreach ($count in $Birds) {
            for ($sample = -2; $sample -lt $Samples; $sample++) {
                $info = [Diagnostics.ProcessStartInfo]::new($exe)
                $info.UseShellExecute = $false
                $info.CreateNoWindow = $true
                $info.RedirectStandardOutput = $true
                $info.RedirectStandardError = $true
                foreach ($argument in @('--bench', "$Frames", $count_option, "$count", '--render', $mode, '--seed', '1')) { $info.ArgumentList.Add($argument) }
                $process = [Diagnostics.Process]::Start($info)
                try {
                    $stdout = $process.StandardOutput.ReadToEndAsync()
                    $stderr = $process.StandardError.ReadToEndAsync()
                    # Process memory counters are unavailable after exit. Sample
                    # the OS high-water mark while it is alive and label it as
                    # observed: the final few milliseconds may be missed.
                    $observedPeak = 0L
                    while (-not $process.WaitForExit(5)) {
                        $process.Refresh()
                        try { $observedPeak = [Math]::Max($observedPeak, $process.PeakWorkingSet64) }
                        catch [InvalidOperationException] { break } # exited between the two calls
                    }
                    $process.WaitForExit()
                    $report = $stdout.GetAwaiter().GetResult()
                    $errorText = $stderr.GetAwaiter().GetResult()
                    if ($process.ExitCode -ne 0) { throw "Benchmark failed: $errorText" }
                    if ($sample -lt 0) { continue }
                    $report | Set-Content -LiteralPath "$out/$mode-$count-$sample.txt" -Encoding utf8NoBOM
                    if ($report -notmatch '(?m)^frame time\s+([0-9.]+)') { throw "Missing frame time: $report" }
                    $ms = [double]::Parse($Matches[1], [Globalization.CultureInfo]::InvariantCulture)
                    $threads = if ($report -match '(?m)^threads\s+(\d+)') { [int]$Matches[1] } else { 1 }
                    if ($report -notmatch '(?m)^bytes/frame\s+(\d+)') { throw "Missing byte count: $report" }
                    $rows.Add([pscustomobject]@{ Render = $mode; Birds = $count; Threads = $threads; Sample = $sample; FrameMs = $ms; BytesPerFrame = [long]$Matches[1]; ObservedPeakWorkingSetBytes = $observedPeak })
                } finally { $process.Dispose() }
            }
        }
    }
    $rows | Export-Csv -LiteralPath "$out/samples.csv" -NoTypeInformation
    $summary = foreach ($group in ($rows | Group-Object Render, Birds)) {
        $times = @($group.Group.FrameMs | Sort-Object)
        $median = ($times[[int][Math]::Floor(($times.Count - 1) / 2)] + $times[[int][Math]::Floor($times.Count / 2)]) / 2
        [pscustomobject]@{ Workload = $group.Name; Threads = $group.Group[0].Threads; MedianMs = $median; MinMs = $times[0]; MaxMs = $times[-1]; BytesPerFrame = $group.Group[0].BytesPerFrame }
    }
    $summary | Format-Table | Out-Host
    $summary | ConvertTo-Json | Set-Content -LiteralPath "$out/summary.json" -Encoding utf8NoBOM
    @("UTC: $([DateTime]::UtcNow.ToString('o'))", "OS: $([Runtime.InteropServices.RuntimeInformation]::OSDescription)", "Architecture: $([Runtime.InteropServices.RuntimeInformation]::OSArchitecture)", "Frames: $Frames", "Samples: $Samples", "Count option: $count_option", (& rustc -vV), (& git rev-parse HEAD)) | Set-Content -LiteralPath "$out/environment.txt" -Encoding utf8NoBOM
    Write-Host "Native measurements: $out. These do not measure emulator painting or prove C parity."
} finally { Pop-Location }
