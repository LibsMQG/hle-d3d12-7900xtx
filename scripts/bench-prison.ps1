<#
.SYNOPSIS
Measures a scripted walk through the prison scene: the same save, settings, keys and route every run.

.DESCRIPTION
Each run copies -State (your save, made standing in the prison cell at the start of the world) to a
fresh data folder, and -Cache (a cache whose shaders and pipelines are already made: `victory shaders`,
one run, `victory warm --used`) to a fresh cache folder, so that no run inherits what an earlier one
made and neither original is written to. It starts the title with the settings of scripts\play.ps1
(-RX7900XT by default) and the panel of numbers in the upper left corner (VICTORY_PANEL), presses
Enter at the start screen, and from -RouteAt seconds on holds the route's keys through the stand-in
pad (VICTORY_KEYS: W A S D the left stick, I J K L the right one). The title runs unattended: do not
touch it.

Every frame's time comes from the frame log (on in the diagnostics build), timed against the log's
"scripted keys" line, so each frame falls into the phase of the route it was drawn in; the first
-SkipS seconds of each phase (the turn from one to the next) are left out. Per phase it gives the
mean fps, the median and 99th-percentile frame time, the 1 % low (the mean of the slowest 1 % of
frames, as fps), the longest frame, the medians of the frame-rate lines (the submitting thread
preparing, its waits for the GPU, draws, draws dropped) and the card's own working time a frame
(its timestamps, VICTORY_CARD_SPANS). A run counts only when the world was there (-WorldDraws
draws a frame) before the route began and the title lasted to its end.

Writes, under -Out\prison-<time>\ (outside the repository: screenshots of the title must never be
committed, docs\asset-policy.md): environment.txt, one folder per run with its log and screenshots,
frames.csv (every frame scored), phases.csv, and summary.md, the table of the phases' medians over
the valid runs.

.EXAMPLE
scripts\bench-prison.ps1 -Exe D:\game\game.exe -State D:\bench\prison-start -Cache D:\bench\cache
scripts\bench-prison.ps1 -Exe D:\game\game.exe -State D:\bench\prison-start -Runs 1 -More @{VICTORY_PANEL='0'}
#>
param(
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][string]$State,
    # A frozen cache copied for every run; without it the configured cache is used as it is.
    [string]$Cache,
    [int]$Runs = 3,
    [ValidateSet('diag', 'release')][string]$Build = 'diag',
    [string]$Console = 'series-x',
    # The settings of scripts\play.ps1 to start from.
    [switch]$Safe,
    [switch]$RayTracing,
    [switch]$NoRX7900XT,
    # Other settings, last: -More @{VICTORY_PANEL='0'}
    [hashtable]$More = @{},
    # Seconds after input starts: Enter at the start screen, then the route.
    [double]$EnterAt = 45,
    [double]$RouteAt = 200,
    [double]$SkipS = 3,
    [int]$WorldDraws = 2000,
    # A screenshot this often during the route (0: none).
    [double]$ShotEvery = 15,
    [string]$Out = (Join-Path $env:LOCALAPPDATA 'victory\bench'),
    # The route: phases one after another, each holding its keys (VICTORY_KEYS names) all through.
    # By default (from the save at the cell window): standing at the window, then turning from it
    # and walking into the dormitory (the camera follows: rooms the card has not drawn yet), the
    # camera turning there, walking on, and walking while turning. Walking toward the window only
    # presses her against it: the first route found that.
    [hashtable[]]$Route = @(
        @{ name = 'stand'; s = 30; keys = @() }
        @{ name = 'into the dormitory'; s = 20; keys = @('S') }
        @{ name = 'look around'; s = 20; keys = @('L') }
        @{ name = 'walk'; s = 20; keys = @('W') }
        @{ name = 'walk and turn'; s = 30; keys = @('W', 'J') }
    )
)
$ErrorActionPreference = 'Stop'
$invariant = [Globalization.CultureInfo]::InvariantCulture
# Every phase must keep frames after its first -SkipS seconds, or nothing of it is measured.
$short = @($route | Where-Object { $_.s -le $SkipS })
if ($short) { throw "phases no longer than -SkipS ($SkipS s) measure nothing: $(($short | ForEach-Object { $_.name }) -join ', ')" }
if ($RouteAt -le $EnterAt) { throw "-RouteAt ($RouteAt) must come after -EnterAt ($EnterAt)" }
$at = $RouteAt
foreach ($p in $route) { $p.from = $at; $at += $p.s; $p.to = $at }
$routeEnd = $at
$keys = @("${EnterAt}:Enter") + @(foreach ($p in $route) { foreach ($k in $p.keys) { '{0}-{1}:{2}' -f $p.from, $p.to, $k } })

$victory = (Resolve-Path (Join-Path $PSScriptRoot "..\target\$Build\victory.exe")).Path
$Exe = (Resolve-Path $Exe).Path
$State = (Resolve-Path $State).Path
if ($Cache) { $Cache = (Resolve-Path $Cache).Path }
$settings = & (Join-Path $PSScriptRoot 'settings.ps1') -Safe:$Safe -RayTracing:$RayTracing -RX7900XT:(!$NoRX7900XT)
# The host's keyboard is not read (VICTORY_HOST_KEYS=0): keys a PC reports held that nobody holds
# would cancel the route's (W and S at once) or add their own.
$settings.VICTORY_PANEL = '1'; $settings.VICTORY_CARD_SPANS = '1'; $settings.VICTORY_LOG = 'info'; $settings.VICTORY_HOST_KEYS = '0'
foreach ($k in $More.Keys) { $settings[$k] = [string]$More[$k] }

# Saves, caches and screenshots are derived from the title: the output may not lie in the
# checkout (docs\asset-policy.md), and it may not overlap what each run copies from (robocopy /MIR
# into a folder inside its source copies it into itself; the originals are only ever read).
$repo = Split-Path $PSScriptRoot
function Get-FullDir([string]$path) { $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($path).TrimEnd('\') + '\' }
function Test-Overlap([string]$a, [string]$b) {
    $a = Get-FullDir $a; $b = Get-FullDir $b
    $a.StartsWith($b, [StringComparison]::OrdinalIgnoreCase) -or $b.StartsWith($a, [StringComparison]::OrdinalIgnoreCase)
}
$outFull = Get-FullDir $Out
if ($outFull.StartsWith((Get-FullDir $repo), [StringComparison]::OrdinalIgnoreCase)) { throw "-Out ($Out) lies in the repository: pick a folder outside it" }
foreach ($source in @(@{ name = 'State'; path = $State }, @{ name = 'Cache'; path = $Cache }) | Where-Object { $_.path }) {
    if (Test-Overlap $Out $source.path) { throw "-Out ($Out) and -$($source.name) ($($source.path)) overlap: pick folders apart" }
}
$session = Join-Path $outFull ('prison-{0:yyyyMMdd-HHmmss}' -f (Get-Date))
New-Item -ItemType Directory -Path $session | Out-Null
$scratch = (New-Item -ItemType Directory -Path (Join-Path $session '_scratch')).FullName

$gpu = Get-CimInstance Win32_VideoController | Where-Object { $_.PNPDeviceID -like 'PCI\*' } |
    Sort-Object AdapterRAM -Descending | Select-Object -First 1
$environment = [ordered]@{
    commit = "$(git -C $repo rev-parse --short HEAD)$(if (git -C $repo status --porcelain --untracked-files=no) { ' (changed)' })"
    build = $Build; gpu = $gpu.Name; driver = $gpu.DriverVersion
    cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()
    ram_gb = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB)
    console = $Console; keys = $keys -join ','
    settings = ($settings.Keys | Sort-Object | ForEach-Object { "$_=$($settings[$_])" }) -join ' '
}
$environment.GetEnumerator() | ForEach-Object { "$($_.Key): $($_.Value)" } | Set-Content (Join-Path $session 'environment.txt')

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class BenchWindow {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@
# The window's picture as the title shows it (PW_CLIENTONLY | PW_RENDERFULLCONTENT), half size.
function Save-Shot($handle, [string]$path) {
    $r = New-Object BenchWindow+RECT
    if (![BenchWindow]::GetClientRect($handle, [ref]$r)) { return }
    $w = [Math]::Max(2, $r.R - $r.L); $h = [Math]::Max(2, $r.B - $r.T)
    $full = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($full); $dc = $g.GetHdc()
    [BenchWindow]::PrintWindow($handle, $dc, 3) | Out-Null
    $g.ReleaseHdc($dc); $g.Dispose()
    $half = New-Object System.Drawing.Bitmap $full, ([int]($w / 2)), ([int]($h / 2))
    $half.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $half.Dispose(); $full.Dispose()
}

function Median([double[]]$xs) {
    if (!$xs) { return $null }
    $s = @($xs | Sort-Object); $i = [int][math]::Floor($s.Count / 2)
    if ($s.Count % 2) { $s[$i] } else { ($s[$i - 1] + $s[$i]) / 2 }
}
# Nearest rank.
function Percentile([double[]]$xs, [double]$p) {
    $s = @($xs | Sort-Object); $s[[math]::Max(0, [math]::Ceiling($s.Count * $p / 100) - 1)]
}
function Parse-Time([string]$s) {
    [datetime]::Parse($s, $invariant, [Globalization.DateTimeStyles]::AssumeUniversal -bor [Globalization.DateTimeStyles]::AdjustToUniversal)
}
function Round1($x) { if ($null -eq $x) { '-' } else { [math]::Round($x, 1) } }

$ansi = [regex]"\x1b\[[0-9;]*m"
$stamp = '^(?<t>\S+)Z\s+INFO '
$keysLine = [regex]($stamp + 'victory_hle::modules::gameinput.*scripted keys \(VICTORY_KEYS\): timed from here')
$timesLine = [regex]($stamp + 'victory_gpu::framelog: frame times: (?<n>\d+) frames in [\d.]+ s: .*each: (?<each>[\d ]+)$')
$rateLine = [regex]($stamp + 'victory_gpu::host: [\d.]+ fps, (?<ms>[\d.]+) ms/frame \((?<prep>[\d.]+) preparing.*?; (?<gpu>[\d.]+) on the GPU;.*?\), (?<draws>\d+) draws/frame, dropped (?<drop>\d+)')
$spanLine = [regex]($stamp + "victory_gpu::host::spans: the card's own stamps per frame: [\d.]+ submissions, working (?<work>[\d.]+) ms")

$phaseRows = [Collections.Generic.List[object]]::new()
Write-Host "prison benchmark: $Runs runs into $session; route from $RouteAt s to $routeEnd s after input starts; do not touch the title"
for ($run = 1; $run -le $Runs; $run++) {
    while (Get-Process -Name victory -ErrorAction SilentlyContinue) { Start-Sleep 1 }
    $dir = (New-Item -ItemType Directory -Path (Join-Path $session ('run{0}' -f $run))).FullName
    $data = Join-Path $scratch 'data'
    if (Test-Path $data) { Remove-Item $data -Recurse -Force }
    Copy-Item $State $data -Recurse
    Remove-Item (Join-Path $data 'launcher.lock') -ErrorAction SilentlyContinue
    $vars = @{} + $settings
    $vars.VICTORY_KEYS = $keys -join ','; $vars.VICTORY_DATA_DIR = $data
    if ($Cache) {
        $vars.VICTORY_CACHE_DIR = Join-Path $scratch 'cache'
        robocopy $Cache $vars.VICTORY_CACHE_DIR /MIR /MT:16 /NFL /NDL /NJH /NJS /NP | Out-Null
        if ($LASTEXITCODE -ge 8) { throw "copying the cache failed (robocopy $LASTEXITCODE)" }
    }
    $log = Join-Path $dir 'run.log'
    $info = [Diagnostics.ProcessStartInfo]::new('cmd.exe', "/c `"`"$victory`" run `"$Exe`" --console $Console --execute 2> `"$log`"`"")
    $info.UseShellExecute = $false
    $info.WorkingDirectory = $repo   # victory.toml is looked for here
    foreach ($k in @($info.EnvironmentVariables.Keys)) {
        if ($k -like 'VICTORY_*' -and $k -notin 'VICTORY_CONFIG', 'VICTORY_GAME_DIR', 'VICTORY_CACHE_DIR') { $info.EnvironmentVariables.Remove($k) }
    }
    foreach ($k in $vars.Keys) { $info.EnvironmentVariables[$k] = $vars[$k] }
    $shell = [Diagnostics.Process]::Start($info)
    $launch = [datetime]::UtcNow
    Write-Host "run $run/$Runs"

    $game = $null; $reader = $null; $start = $null; $world = $null; $outcome = 'timed out'; $past = $false
    $frames = [Collections.Generic.List[object]]::new(); $rates = [Collections.Generic.List[object]]::new()
    $spans = [Collections.Generic.List[object]]::new()
    $nextShot = $RouteAt + $ShotEvery / 2
    try {
        while (([datetime]::UtcNow - $launch).TotalSeconds -lt $routeEnd + 240) {
            Start-Sleep -Milliseconds 250
            if (!$game) { $game = Get-CimInstance Win32_Process -Filter "ParentProcessId = $($shell.Id) AND Name = 'victory.exe'" | Select-Object -First 1 }
            if (!$reader) {
                if (Test-Path $log) { $reader = [IO.StreamReader]::new([IO.FileStream]::new($log, 'Open', 'Read', 'ReadWrite, Delete')) }
                elseif ($shell.HasExited) { $outcome = 'exited before logging'; break }
                else { continue }
            }
            while ($null -ne ($line = $reader.ReadLine())) {
                $line = $ansi.Replace($line, '')
                if ($line -like 'VIctory exited with*' -or $line -like 'VIctory: unhandled exception*' -or $line -like 'VIctory: host GPU failure*') { $outcome = $line }
                if (!$start -and ($m = $keysLine.Match($line)).Success) { $start = Parse-Time $m.Groups['t'].Value }
                elseif ($start -and ($m = $timesLine.Match($line)).Success) {
                    # The line closes its frames: the last ended at its time, each earlier one a frame before.
                    $end = ((Parse-Time $m.Groups['t'].Value) - $start).TotalSeconds
                    if ($end -gt $routeEnd) { $past = $true }
                    $each = @($m.Groups['each'].Value.Trim() -split ' +' | ForEach-Object { [double]$_ })
                    for ($i = $each.Count - 1; $i -ge 0; $i--) { $frames.Add([pscustomobject]@{ t = $end; ms = $each[$i] }); $end -= $each[$i] / 1e3 }
                } elseif ($start -and ($m = $rateLine.Match($line)).Success) {
                    $r = [pscustomobject]@{ t = ((Parse-Time $m.Groups['t'].Value) - $start).TotalSeconds; prep = [double]$m.Groups['prep'].Value
                        wait = [double]$m.Groups['gpu'].Value; draws = [double]$m.Groups['draws'].Value; drop = [double]$m.Groups['drop'].Value }
                    $rates.Add($r)
                    if (!$world -and $r.draws -ge $WorldDraws) { $world = $r.t; Write-Host ('  world at {0:N0} s' -f $world) }
                } elseif ($start -and ($m = $spanLine.Match($line)).Success) {
                    $spans.Add([pscustomobject]@{ t = ((Parse-Time $m.Groups['t'].Value) - $start).TotalSeconds; work = [double]$m.Groups['work'].Value })
                }
            }
            if ($outcome -ne 'timed out') { break }
            if ($shell.HasExited -and $reader.EndOfStream) { $outcome = "exited ($($shell.ExitCode))"; break }
            if (!$start) { continue }
            $now = ([datetime]::UtcNow - $start).TotalSeconds
            if ($ShotEvery -gt 0 -and $game -and $now -ge $nextShot -and $now -lt $routeEnd) {
                $p = Get-Process -Id $game.ProcessId -ErrorAction SilentlyContinue
                if ($p -and $p.MainWindowHandle -ne 0) { Save-Shot $p.MainWindowHandle (Join-Path $dir ('t{0:000}.png' -f [int]$now)) }
                $nextShot += $ShotEvery
            }
            # The frame log closes a frame's line some seconds after it: done once a line is past the route.
            if ($past) { $outcome = 'route done'; break }
        }
    } finally {
        if ($reader) { $reader.Dispose() }
        if ($game) { Stop-Process -Id $game.ProcessId -Force -ErrorAction SilentlyContinue }
        if (!$shell.HasExited) { $shell.Kill() }
    }

    $problems = @()
    if ($outcome -ne 'route done') { $problems += $outcome }
    if (!$start) { $problems += 'no scripted keys line in the log' }
    if (!$world) { $problems += 'the world never came' } elseif ($world -gt $RouteAt) { $problems += ('the world came at {0:N0} s, after the route began' -f $world) }
    $rows = foreach ($p in $route) {
        $fs = @($frames | Where-Object { $_.t -gt $p.from + $SkipS -and $_.t -le $p.to })
        $rs = @($rates | Where-Object { $_.t -gt $p.from + $SkipS -and $_.t -le $p.to })
        $ss = @($spans | Where-Object { $_.t -gt $p.from + $SkipS -and $_.t -le $p.to })
        $fs | ForEach-Object { [pscustomobject]@{ run = $run; phase = $p.name; t = [math]::Round($_.t, 3); ms = $_.ms } } |
            Export-Csv (Join-Path $session 'frames.csv') -Append -NoTypeInformation
        $ms = [double[]]@($fs.ms)
        # (A frame-rate line comes every second or so: a phase without one was not measured.)
        if (!$ms -or !$rs) { $problems += "phase '$($p.name)' has $($ms.Count) frames and $($rs.Count) frame-rate lines" }
        $slow = @($ms | Sort-Object -Descending | Select-Object -First ([math]::Max(1, [int][math]::Ceiling($ms.Count / 100))))
        [pscustomobject]@{
            run = $run; valid = $false; problems = ''; phase = $route.IndexOf($p); name = $p.name; frames = $ms.Count
            fps = $(if ($ms) { $ms.Count * 1e3 / ($ms | Measure-Object -Sum).Sum })
            median_ms = Median $ms; p99_ms = $(if ($ms) { Percentile $ms 99 })
            low1_fps = $(if ($ms) { 1e3 / ($slow | Measure-Object -Average).Average }); longest_ms = ($ms | Measure-Object -Maximum).Maximum
            prep_ms = Median $rs.prep; wait_gpu_ms = Median $rs.wait; card_ms = Median $ss.work; draws = Median $rs.draws; dropped = Median $rs.drop
        }
    }
    $valid = !$problems
    Write-Host $(if ($valid) { '  valid' } else { "  INVALID: $($problems -join '; ')" })
    foreach ($row in $rows) {
        $row.valid = $valid; $row.problems = $problems -join '; '
        $phaseRows.Add($row)
        $row | Export-Csv (Join-Path $session 'phases.csv') -Append -NoTypeInformation
    }
}
Remove-Item $scratch -Recurse -Force -ErrorAction SilentlyContinue

# Per phase, the median over the valid runs.
$valid = @($phaseRows | Where-Object valid)
$runsValid = @($valid | Select-Object -ExpandProperty run -Unique).Count
$md = @(
    "Prison route, $runsValid of $Runs runs valid; median over them. $($environment.gpu) ($($environment.driver)), $($environment.cpu), $($environment.ram_gb) GB, $Build build at $($environment.commit), $Console."
    ''
    '| Phase | Keys | fps | Median ms | P99 ms | 1% low fps | Longest ms | Preparing ms | Waiting for GPU ms | GPU working ms | Draws | Dropped |'
    '|---|---|---|---|---|---|---|---|---|---|---|---|'
)
foreach ($p in $route) {
    $g = @($valid | Where-Object { $_.name -eq $p.name })
    $stat = { param($k) Round1 (Median ([double[]]@($g | ForEach-Object { $_.$k } | Where-Object { $null -ne $_ }))) }
    $md += '| {0} | {1} | {2} | {3} | {4} | {5} | {6} | {7} | {8} | {9} | {10} | {11} |' -f $p.name, $(if ($p.keys) { $p.keys -join '+' } else { '-' }),
        (& $stat 'fps'), (& $stat 'median_ms'), (& $stat 'p99_ms'), (& $stat 'low1_fps'), (& $stat 'longest_ms'),
        (& $stat 'prep_ms'), (& $stat 'wait_gpu_ms'), (& $stat 'card_ms'), (& $stat 'draws'), (& $stat 'dropped')
}
$md | Set-Content (Join-Path $session 'summary.md') -Encoding UTF8
$md | Write-Host
Write-Host "results: $session"
if ($runsValid -lt $Runs) { exit 1 }
