<#
.SYNOPSIS
  Runs the diagnostics build once for a profiling pass and keeps its output.

.DESCRIPTION
  Two kinds of pass, run them separately (GPU timing waits on every GPU run,
  so it distorts the CPU picture):

    -Mode cpu  VICTORY_PROFILE: samples the submitting thread every ms and the
               other threads every 16 ms; the hottest functions and stacks are
               rewritten to cpu-profile.txt every 20 s.
    -Mode gpu  VICTORY_GPU_TIMING: GPU ms per frame by pipeline, in the log.

  Output goes to %LOCALAPPDATA%\victory\profiles\<date>-<mode>\ :
    log.txt (stderr), cpu-profile.txt (cpu mode), env.txt (settings used).

  Test protocol: load the save, stand still in the same heavy spot for about
  90 seconds (camera untouched), then quit. Note the fps in the window caption.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\scripts\profile-run.ps1 -Game "C:\path\to\game_scarlett_master.exe" -Mode cpu
#>
param(
    [Parameter(Mandatory = $true)][string]$Game,
    [ValidateSet('cpu', 'gpu', 'plain')][string]$Mode = 'cpu',
    [string]$Console = 'series-x',
    [string]$Ambient = ''
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $repo 'target\diag\victory.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "No diagnostics build at $exe. Build it with: cargo build --profile diag -p victory --features diagnostics" }
if (-not (Test-Path -LiteralPath $Game)) { throw "Game executable not found: $Game" }

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$out = Join-Path $env:LOCALAPPDATA "victory\profiles\$stamp-$Mode"
New-Item -ItemType Directory -Force $out | Out-Null

# Info-level log plus the memory line every 10 s.
$env:VICTORY_LOG = 'info'
$env:VICTORY_MEMORY_LOG_SECS = '10'
$env:VICTORY_HAIR = '1'
Remove-Item Env:VICTORY_PROFILE, Env:VICTORY_GPU_TIMING -ErrorAction SilentlyContinue
switch ($Mode) {
    'cpu' { $env:VICTORY_PROFILE = Join-Path $out 'cpu-profile.txt' }
    'gpu' { $env:VICTORY_GPU_TIMING = '1' }
}
if ($Ambient) { $env:VICTORY_AMBIENT = $Ambient }

# Shader cache entries are keyed by the executable. Keying them by a kept
# copy instead lets rebuilds that do not touch shader translation skip the
# minutes of re-translating. Delete target\diag\victory-shaders.exe after a
# change to crates\victory-shader or the translation in crates\victory-gpu.
$shaderKey = Join-Path $repo 'target\diag\victory-shaders.exe'
Remove-Item Env:VICTORY_SHADER_BUILD_OF -ErrorAction SilentlyContinue
if (-not (Test-Path -LiteralPath $shaderKey)) { Copy-Item -LiteralPath $exe $shaderKey }
$env:VICTORY_SHADER_BUILD_OF = $shaderKey

Get-ChildItem Env: | Where-Object Name -like 'VICTORY_*' |
    ForEach-Object { "$($_.Name)=$($_.Value)" } | Out-File -Encoding utf8 (Join-Path $out 'env.txt')

Write-Host "Profiling pass '$Mode' -> $out"
Write-Host 'Load in, stand still in one heavy spot ~90 s, then quit the game.'
$p = Start-Process -FilePath $exe `
    -ArgumentList @('run', "`"$Game`"", '--execute', '--console', $Console) `
    -RedirectStandardError (Join-Path $out 'log.txt') `
    -RedirectStandardOutput (Join-Path $out 'stdout.txt') `
    -PassThru -NoNewWindow
$p.WaitForExit()
Write-Host "Exited with code $($p.ExitCode). Results in: $out"
