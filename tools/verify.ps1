#requires -Version 7.0
# Native Windows gates. -AllLocal adds the Unix gates in the chosen WSL distro.
[CmdletBinding()]
param([switch]$AllLocal, [string]$Distribution = 'Debian')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not $IsWindows) {
    $unixArgs = if ($AllLocal) { @('--all-local') } else { @() }
    & "$PSScriptRoot/unix.ps1" verify @unixArgs
    return
}
$failed = [Collections.Generic.List[string]]::new()
function Native {
    $command, $arguments = $args
    & $command @arguments
    if ($LASTEXITCODE -ne 0) { throw "$command failed ($LASTEXITCODE)" }
}
function Gate([string]$Name, [scriptblock]$Action) {
    Write-Host "== $Name"
    try { & $Action; Write-Host "PASS $Name" }
    catch { $failed.Add($Name); Write-Host "FAIL ${Name}: $_" }
}
function Linkage([string]$Binary) {
    $dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1
    if (-not $dumpbin) {
        $vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
        if (Test-Path -LiteralPath $vswhere) {
            $installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
            if ($installation) {
                $dumpbin = Get-ChildItem -Path "$installation/VC/Tools/MSVC/*/bin/Hostx64/x64/dumpbin.exe" |
                    Sort-Object FullName -Descending | Select-Object -ExpandProperty FullName -First 1
            }
        }
    }
    if (-not $dumpbin) { throw 'dumpbin.exe not found; install the Visual C++ build tools for the import audit.' }
    $output = Native $dumpbin @('/nologo', '/dependents', $Binary)
    $libraries = @($output | ForEach-Object { if ($_ -match '^\s+([\w.-]+\.dll)\s*$') { $Matches[1] } })
    if (-not $libraries.Count) { throw 'No PE imports found; linkage audit did not run.' }
    foreach ($library in $libraries) {
        Write-Host "  $library"
        if ($library -notmatch '^(api-ms-win-[\w-]+|KERNEL32|ntdll|ADVAPI32|USERENV|WS2_32|bcrypt|bcryptprimitives|ucrtbase|VCRUNTIME140(?:_1)?)\.dll$') {
            throw "Unexpected native dependency: $library"
        }
    }
}
Push-Location $root
try {
    $target = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR, $root) } else { Join-Path $root 'target' }
    Gate 'toolchain pin' {
        $version = Native rustc @('--version')
        Write-Host $version
        if ($version -notlike 'rustc 1.96.0 *') { throw 'Expected pinned Rust 1.96.0' }
        Native cargo @('--version')
    }
    Gate 'formatting' { Native cargo @('fmt', '--all', '--', '--check') }
    Gate 'check' { Native cargo @('check', '--all-targets', '--all-features', '--locked', '--offline') }
    Gate 'clippy' { Native cargo @('clippy', '--all-targets', '--all-features', '--locked', '--offline', '--', '-D', 'warnings') }
    Gate 'tests (debug)' { Native cargo @('test', '--all-targets', '--all-features', '--locked', '--offline') }
    Gate 'tests (release)' { Native cargo @('test', '--release', '--all-targets', '--all-features', '--locked', '--offline') }
    Gate 'release build' { Native cargo @('build', '--release', '--locked', '--offline') }
    Gate 'dependency graph' {
        $tree = @(Native cargo @('tree', '--locked', '--offline', '--edges', 'all', '--target', 'all', '--all-features', '--prefix', 'none'))
        if ($tree.Count -ne 1 -or $tree[0] -notmatch '^rbirds v') { throw "Unexpected dependency graph: $tree" }
        if (Select-String -Path Cargo.toml -Pattern '^\[(target\..*\.)?(dev-|build-)?dependencies') { throw 'Dependency tables are not permitted' }
    }
    Gate 'unsafe confined to src/platform' {
        $hits = Get-ChildItem -LiteralPath src -Recurse -Filter *.rs |
            Where-Object { $_.FullName -notlike "$root\src\platform\*" } |
            Select-String -Pattern 'unsafe' | Where-Object { $_.Line -notmatch '(forbid|deny|allow)\(unsafe_code\)' }
        if ($hits) { throw ($hits | Out-String) }
    }
    Gate 'native linkage' { Linkage (Join-Path $target 'release/rbirds.exe') }
    Gate 'install and uninstall' {
        $prefix = Join-Path $target "install-smoke-$([Guid]::NewGuid().ToString('N'))"
        Native cargo @('install', '--path', '.', '--locked', '--offline', '--root', $prefix, '--quiet')
        Native (Join-Path $prefix 'bin/rbirds.exe') @('--version')
        Native cargo @('uninstall', '--root', $prefix, 'rbirds', '--quiet')
        if (Test-Path -LiteralPath "$prefix/bin/rbirds.exe") { throw 'Uninstall left the binary behind' }
        # Keep Cargo's tiny metadata directory for inspection; no recursive deletion.
    }
    if ($AllLocal) {
        Gate "Unix gates (WSL $Distribution)" { & "$PSScriptRoot/unix.ps1" verify -Distribution $Distribution }
    } else {
        Write-Host 'UNAVAILABLE Unix C comparisons / POSIX PTY tests: use -AllLocal with WSL, or native Unix CI.'
    }
    Write-Host 'UNAVAILABLE real-terminal appearance: manual checks in docs/WINDOWS.md.'
    Write-Host 'UNAVAILABLE performance budgets: tools/perf.ps1 on a quiet host; Unix C budgets use tools/perf.sh.'
    if ($failed.Count) { throw "Failed gates: $($failed -join ', ')" }
    Write-Host 'All requested automated gates passed.'
} finally { Pop-Location }
