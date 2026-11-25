<#
.SYNOPSIS
  Unattended in-world benchmark of the diagnostics build.

.DESCRIPTION
  Launches the title, presses Enter once the title screen is up (found in the
  log: light frames), waits through the loading screen and the opening
  cinematic, measures -MeasureSeconds in the world with no input, then ends
  the process and writes summary.txt next to the log.

  Output: %LOCALAPPDATA%\victory\bench\<date>-<label>\ (log.txt, cpu-profile.txt,
  summary.txt). The summary averages the 5-second frame lines of the window.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\scripts\bench.ps1 -Game "C:\path\game_scarlett_master.exe" -Label baseline
#>
param(
    [Parameter(Mandatory = $true)][string]$Game,
    [string]$Label = 'run',
    [int]$CinematicSeconds = 150,
    [int]$MeasureSeconds = 90,
    [int]$TimeoutSeconds = 1200,
    [switch]$GpuTiming,
    [switch]$Async,
    [int]$Shots = 3,
    [int]$CinematicShotEvery = 0,
    [switch]$DumpTargets,
    [string[]]$ExtraEnv = @(),
    [string]$Exe = ''
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $repo 'target\diag\victory.exe' }
if (-not (Test-Path -LiteralPath $Exe)) { throw "No build at $Exe" }
if (-not (Test-Path -LiteralPath $Game)) { throw "Game executable not found: $Game" }
if (Get-Process victory -ErrorAction SilentlyContinue) { throw 'victory.exe is already running; close it first.' }

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$out = Join-Path $env:LOCALAPPDATA "victory\bench\$stamp-$Label"
New-Item -ItemType Directory -Force $out | Out-Null
$keys = Join-Path $out 'keys.txt'
$log = Join-Path $out 'log.txt'
New-Item -ItemType File $keys | Out-Null

foreach ($v in 'VICTORY_PROFILE', 'VICTORY_GPU_TIMING', 'VICTORY_ASYNC_SUBMIT', 'VICTORY_KEYS', 'VICTORY_CAPTURE', 'VICTORY_CAPTURE_FRAMES') {
    Remove-Item "Env:$v" -ErrorAction SilentlyContinue
}
$env:VICTORY_LOG = 'info'
$env:VICTORY_HAIR = '1'
$env:VICTORY_MEMORY_LOG_SECS = '10'
$env:VICTORY_KEYS_FILE = $keys
if ($Async) { $env:VICTORY_ASYNC_SUBMIT = '1' }
if ($DumpTargets) { $env:VICTORY_DUMP_TARGETS = '1' } else { Remove-Item Env:VICTORY_DUMP_TARGETS -ErrorAction SilentlyContinue }
foreach ($kv in $ExtraEnv) { $k, $v = $kv -split '=', 2; Set-Item "Env:$k" $v }
# Screenshots: a `want` file in the folder saves the next frame.
$shotsDir = Join-Path $out 'frames'
if ($Shots -gt 0 -or $CinematicShotEvery -gt 0) { New-Item -ItemType Directory -Force $shotsDir | Out-Null; $env:VICTORY_CAPTURE = $shotsDir }
$shotTimes = @(); if ($Shots -gt 0) { $shotTimes = 0..($Shots - 1) | ForEach-Object { [int](5 + $_ * ($MeasureSeconds - 10) / [Math]::Max(1, $Shots - 1)) } }
$shotsTaken = 0
$cinematicShots = 0
Remove-Item Env:VICTORY_D3D12_DRED -ErrorAction SilentlyContinue
# GPU timing per pipeline needs the commands noted, which DRED turns on.
if ($GpuTiming) { $env:VICTORY_GPU_TIMING = '1'; $env:VICTORY_D3D12_DRED = '1' } else { $env:VICTORY_PROFILE = Join-Path $out 'cpu-profile.txt' }
$shaderKey = Join-Path $repo 'target\diag\victory-shaders.exe'
if (-not (Test-Path -LiteralPath $shaderKey)) { Copy-Item -LiteralPath $Exe $shaderKey }
$env:VICTORY_SHADER_BUILD_OF = $shaderKey

# One frame line: fps, ms, preparing, copies, textures, plans, gpu, back, records, draws.
$frameRe = '([\d.]+) fps, ([\d.]+) ms/frame \(([\d.]+) preparing: copies ([\d.]+) ms [\d.]+ MB, textures ([\d.]+) ms [\d.]+ MB, bindless plans ([\d.]+) ms; ([\d.]+) on the GPU; ([\d.]+) copying back [\d.]+ MB; bindless records ([\d.]+) ms\), (\d+) draws/frame'

function Read-Log {
    if (-not (Test-Path -LiteralPath $log)) { return @() }
    $fs = [System.IO.File]::Open($log, 'Open', 'Read', 'ReadWrite')
    try { $text = (New-Object System.IO.StreamReader($fs)).ReadToEnd() } finally { $fs.Close() }
    return ($text -replace ([string][char]27 + '\[[0-9;]*m'), '') -split "`r?`n"
}

function Get-Frames([string[]]$lines) {
    foreach ($l in $lines) {
        if ($l -match $frameRe) {
            [pscustomobject]@{
                Time = [datetime]::Parse($l.Substring(0, 27), $null, 'RoundtripKind')
                Fps = [double]$Matches[1]; Ms = [double]$Matches[2]; Prep = [double]$Matches[3]
                Copies = [double]$Matches[4]; Textures = [double]$Matches[5]; Plans = [double]$Matches[6]
                Gpu = [double]$Matches[7]; Back = [double]$Matches[8]; Records = [double]$Matches[9]
                Draws = [int]$Matches[10]
            }
        }
    }
}

function Write-Status([string]$text) { Write-Host ("[{0:HH:mm:ss}] {1}" -f (Get-Date), $text) }

$p = Start-Process -FilePath $Exe -ArgumentList @('run', "`"$Game`"", '--execute', '--console', 'series-x') `
    -RedirectStandardError $log -RedirectStandardOutput (Join-Path $out 'stdout.txt') -PassThru -WindowStyle Minimized
$started = Get-Date
$phase = 'boot'; $pressed = $null; $heavySince = $null; $measureFrom = $null
try {
    while (-not $p.HasExited) {
        Start-Sleep -Seconds 2
        if (((Get-Date) - $started).TotalSeconds -gt $TimeoutSeconds) { Write-Status 'timeout'; break }
        $frames = @(Get-Frames (Read-Log))
        $last = $frames | Select-Object -Last 1
        switch ($phase) {
            'boot' {
                # Title screen: light frames (few draws) at a high rate.
                $light = @($frames | Select-Object -Last 2 | Where-Object { $_.Draws -le 30 -and $_.Fps -gt 60 })
                if ($light.Count -ge 2) {
                    Add-Content -LiteralPath $keys 'Enter'
                    $pressed = Get-Date; $phase = 'loading'; Write-Status 'title screen: Enter pressed'
                }
            }
            'loading' {
                if ($last -and $last.Draws -ge 300 -and $last.Time -gt $pressed.ToUniversalTime()) {
                    $heavySince = Get-Date; $phase = 'cinematic'; Write-Status 'world drawing: waiting through the cinematic'
                } elseif (((Get-Date) - $pressed).TotalSeconds -gt 45 -and @($frames | Select-Object -Last 1 | Where-Object { $_.Draws -le 30 -and $_.Fps -gt 60 }).Count -gt 0) {
                    # Still on the title screen: press again.
                    Add-Content -LiteralPath $keys 'Enter'
                    $pressed = Get-Date; Write-Status 'still on the title screen: Enter again'
                }
            }
            'cinematic' {
                if ($CinematicShotEvery -gt 0) {
                    $since = ((Get-Date) - $heavySince).TotalSeconds
                    if ($since -ge ($cinematicShots + 1) * $CinematicShotEvery) {
                        New-Item -ItemType File -Force (Join-Path $shotsDir 'want') | Out-Null
                        $cinematicShots++
                    }
                }
                if (((Get-Date) - $heavySince).TotalSeconds -ge $CinematicSeconds) {
                    $measureFrom = (Get-Date).ToUniversalTime(); $phase = 'measure'; Write-Status "measuring $MeasureSeconds s"
                }
            }
            'measure' {
                $elapsed = ((Get-Date).ToUniversalTime() - $measureFrom).TotalSeconds
                if ($shotsTaken -lt $shotTimes.Count -and $elapsed -ge $shotTimes[$shotsTaken]) {
                    $name = if ($DumpTargets -and $shotsTaken -eq 0) { 'wantall' } else { 'want' }
                    New-Item -ItemType File -Force (Join-Path $shotsDir $name) | Out-Null
                    $shotsTaken++
                }
                if (((Get-Date).ToUniversalTime() - $measureFrom).TotalSeconds -ge $MeasureSeconds) { $phase = 'done'; break }
            }
        }
        if ($phase -eq 'done') { break }
    }
} finally {
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force; Start-Sleep -Seconds 2 }
}

$frames = @(Get-Frames (Read-Log))
$window = @($frames | Where-Object { $measureFrom -and $_.Time -gt $measureFrom.AddSeconds(5) })
$summary = New-Object System.Collections.Generic.List[string]
$summary.Add("label: $Label  phase reached: $phase  lines in window: $($window.Count)")
if ($window.Count -gt 0) {
    $avg = { param($name) ($window | Measure-Object -Property $name -Average).Average }
    $totalFrames = ($window | ForEach-Object { $_.Fps * 5 } | Measure-Object -Sum).Sum
    $totalSecs = $window.Count * 5
    $summary.Add(('fps: {0:N2} (frames/time)  min line {1:N1}  max line {2:N1}' -f ($totalFrames / $totalSecs), ($window | Measure-Object Fps -Minimum).Minimum, ($window | Measure-Object Fps -Maximum).Maximum))
    $summary.Add(('ms/frame {0:N1}: preparing {1:N1} (copies {2:N1}, textures {3:N1}, plans {4:N1}), gpu {5:N1}, back {6:N1}, records {7:N1}, draws {8:N0}' -f `
        (& $avg Ms), (& $avg Prep), (& $avg Copies), (& $avg Textures), (& $avg Plans), (& $avg Gpu), (& $avg Back), (& $avg Records), (& $avg Draws)))
}
$text = Read-Log
$snap = $text | Where-Object { $_ -match 'sparse writes since' } | Select-Object -Last 3
$summary.AddRange([string[]]@($snap | ForEach-Object { ($_ -replace '^.*sparse writes since: ', 'sparse: ') }))
$summary | Set-Content -Encoding utf8 (Join-Path $out 'summary.txt')
$summary | ForEach-Object { Write-Host $_ }
Write-Host "Results in: $out"
