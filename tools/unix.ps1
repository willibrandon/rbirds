#requires -Version 7.0
# Access Unix C-oracle, Docker and VHS workflows from Windows through WSL.
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)]
    [ValidateSet('verify', 'reference', 'perf', 'linux', 'vhs')][string]$Task,
    [string]$Distribution = 'Debian',
    [Parameter(ValueFromRemainingArguments)][string[]]$TaskArguments
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$script = if ($Task -eq 'vhs') { 'tools/vhs/live.sh' } else { "tools/$Task.sh" }
if ($IsWindows) {
    # --cd accepts Windows paths; arguments are passed directly, not shell text.
    & wsl.exe --distribution $Distribution --cd $root --exec sh -c 'export PATH="$HOME/.cargo/bin:$PATH"; exec "$@"' rbirds env CARGO_TARGET_DIR=/tmp/rbirds-wsl-target sh $script @TaskArguments
} else {
    Push-Location $root
    try { & sh $script @TaskArguments } finally { Pop-Location }
}
if ($LASTEXITCODE -ne 0) { throw "$Task failed ($LASTEXITCODE)" }
