# hle-d3d12-7900xtx launcher: presets, switches and settings by libs.
# Starts the title with the settings this fork is played with (README.md, "How to run it").
#   scripts\play.ps1 -Exe <path of your own copy's executable>
# -Safe         without the newer, faster ways (the first thing to try when something is wrong)
# -RayTracing   the title's own ray-traced indirect light: lit interiors, about a third slower
# -Build        target\diag (the default, the build all measurements are of) or target\release
# -Tuned        also the settings measured best, sized to this PC: our helper threads by its
#               logical processors, memory kept in place by its RAM, the card buffers by its
#               VRAM (the build's own scaling). The one to start from on any PC.
# -RX7900XTX    -Tuned with the exact values measured on the reference PC (RX 7900 XTX 24 GB,
#               Ryzen 9 5900X, 32 GB of RAM; README.md), whatever this PC is; -RX7900XT is
#               the same switch under its old name
# -Threads      with -Tuned: size our threads as for this many logical processors (default: this PC's)
# -RamGB        with -Tuned: size memory kept in place as for this much RAM (default: this PC's)
# -SkipMovies   skips the opening movies (off by default): once (logos and intros), all (looping
#               menu backgrounds too, which then stay dark); VICTORY_SKIP_BINK
# -Fps          frames a second for light frames, those of at most 500 draws (videos, loading
#               screens, the pause menu, the map), in place of the title's 30: 60 by default
#               (not with -Safe), 0 for no limit, off for the title's 30. The world stays at
#               the title's 30; VICTORY_FPS
# -Warm         the caches made ahead before the title starts, with this line's settings:
#               auto (the default) translates every shader of the title's folder only when
#               the shader cache key changed (a new translator, other settings, another
#               card; a few minutes), then makes the host pipelines earlier runs drew with
#               that the driver's cache does not have yet (`victory warm --used`); always
#               translates again whatever the key; off starts the title at once
# -List         only print the settings this line would start with, and stop
# -More         other settings: -More @{VICTORY_PANEL='1'}
# Run it from a normal window, not as administrator: the driver then keeps its shader cache elsewhere.
param(
    [Parameter(Mandatory)][string]$Exe,
    [switch]$Safe,
    [switch]$RayTracing,
    [switch]$Tuned,
    [Alias('RX7900XT')][switch]$RX7900XTX,
    [ValidateRange(1, 256)][int]$Threads = 0,
    [ValidateRange(1, 1024)][int]$RamGB = 0,
    [switch]$List,
    [ValidateSet('once', 'all')][string]$SkipMovies = '',
    [ValidateSet('diag', 'release')][string]$Build = 'diag',
    [ValidatePattern('^(\d{1,4}|off)$')][string]$Fps = '60',
    [ValidateSet('auto', 'always', 'off')][string]$Warm = 'auto',
    [hashtable]$More = @{}
)
# VICTORY_CARD_AWAKE (the card's clocks kept up between submissions, host/awake.rs) is
# left off: it keeps the card busy all the time (100 % in Task Manager, about 100 W
# more). -More @{VICTORY_CARD_AWAKE='1'} turns it on.
$settings = @{
    VICTORY_TEXTURE_NO_WAIT = '1'; VICTORY_RELAY = '2'; VICTORY_WIDE_BACK = '1'; VICTORY_CARD_REHASH_RUNS = '1'
    VICTORY_JOIN_COPIES = '1'; VICTORY_INDIRECT_WINDOW = '1'; VICTORY_RELAY_COPY = '1'; VICTORY_CARD_STEPS = '2'
    VICTORY_INDIRECT_BATCH = '2'; VICTORY_MAP_THREADS = '1'; VICTORY_TITLE_PRIORITIES = '0'
    VICTORY_SHOW_AFTER = '1'; VICTORY_AMBIENT = '0.8'; VICTORY_D3D12_DRED = 'light'; VICTORY_SKIP_PIPELINES = 'none'
}
if ($Safe) {
    $settings.VICTORY_UNDER_WIDE = '0'; $settings.VICTORY_LAND_ECHO = '0'
} else {
    $settings.VICTORY_THIN = '1'; $settings.VICTORY_JOIN_PARTS = '8'; $settings.VICTORY_UPLOAD_AHEAD = '1'
}
if ($RayTracing) { $settings.VICTORY_BVH_BUILDER = '1' }
if ($SkipMovies) { $settings.VICTORY_SKIP_BINK = $(if ($SkipMovies -eq 'all') { 'all' } else { '1' }) }
# Light frames at up to 60 a second (the title's own limiter let through for them, its
# simulation's shortest step following): -Safe and -Fps off leave the title's 30.
if ($Fps -ne 'off' -and -not ($Safe -and -not $PSBoundParameters.ContainsKey('Fps'))) { $settings.VICTORY_FPS = $Fps }
if ($RX7900XTX) { $Tuned = $true }
$machine = ''
if ($Tuned) {
    # Less committed memory in windows of title memory (about 5 GB less); smaller texture
    # steps at scene cuts; windows in private memory used in place; pipelines written down
    # for "victory warm".
    $settings.VICTORY_ROOT_WINDOW_MB = '12'; $settings.VICTORY_WINDOW_KB = '1024'
    $settings.VICTORY_TEXTURE_STAGE_MS = '80'
    $settings.VICTORY_INPLACE = '1'
    $settings.VICTORY_PIPELINE_BOOK = '1'
    # Measured for 20 fps on the 7800X3D (CHANGELOG.md): the write watch asked
    # early, the heaps' records every 1 ms, longer joined submissions, and the
    # title's idle threads at below normal (16 threads are not enough for them
    # to wait their turn).
    $settings.VICTORY_ASK_EARLY = '2'; $settings.VICTORY_HEAP_SYNC_EARLY = '1'
    # (-Safe keeps one part per joined submission: it is the line to start from when something is wrong.)
    if (-not $Safe) { $settings.VICTORY_JOIN_PARTS = '16' }; $settings.VICTORY_JOIN_LISTS = '96'
    $settings.VICTORY_TITLE_PRIORITIES = '1'
    # Our threads that translate list calls, compare title bytes and land
    # copies, sized to the processor so that the title's own threads are not
    # starved. 8/3/3 from 24 logical processors: in-run A/B on a Ryzen 9 5900X
    # with VICTORY_CCD=near (the translators on the core complex without the
    # title), jail, standing, two runs: eight translators against four, frames
    # -0.8% and -3.4% (median against the neighbours), p90 -7% and -9%, the
    # submitting thread's wait for lists still being translated 24-26 -> 11 ms
    # a frame; the translators' work grows by a third (one list is translated
    # by one thread, and the longest still takes ~30 ms). 4/3/3 from 16 (8 and
    # 6 were too many there: the title's own threads ran short, free roam 6-13
    # ms less preparing a frame, CHANGELOG.md). Fewer logical processors get
    # fewer of ours: 3/2/2 from 12, 2/1/1 from 8, 1/1/1 under 8 (scaled by the
    # same rule, not measured).
    if ($RX7900XTX) {
        $thin, $under, $land = 8, 3, 3
    } else {
        $n = $(if ($Threads) { $Threads } else { [Environment]::ProcessorCount })
        $thin, $under, $land = $(if ($n -ge 24) { 8, 3, 3 } elseif ($n -ge 16) { 4, 3, 3 } elseif ($n -ge 12) { 3, 2, 2 } elseif ($n -ge 8) { 2, 1, 1 } else { 1, 1, 1 })
        $machine += "$n threads"
    }
    $settings.VICTORY_THIN_THREADS = "$thin"; $settings.VICTORY_UNDER_THREADS = "$under"
    $settings.VICTORY_LAND_THREADS = "$land"
    # Memory kept in place (VICTORY_INPLACE_MB: opening locks the pages into the
    # PC's memory): 3072 MB measured with 32 GB of RAM; 1536 with 16-31 GB, 768
    # under 16 (not measured).
    if ($RX7900XTX) {
        $settings.VICTORY_INPLACE_MB = '3072'
    } else {
        $gb = $(if ($RamGB) { $RamGB } else { [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB) })
        $settings.VICTORY_INPLACE_MB = $(if ($gb -ge 32) { '3072' } elseif ($gb -ge 16) { '1536' } else { '768' })
        $machine += ", $gb GB RAM"
    }
    # Copies of title buffers in the card's memory (shader-write compares on the
    # card): the reference PC's 24 GB card takes buffers of 1 MiB and more, up to
    # 1536 MB. Elsewhere the build sizes them by the card's VRAM budget itself
    # (16 MiB / 512 MB up to 12 GB, 4 MiB / 2048 MB on 16 GB, 1 MiB / up to
    # 8192 MB from 20 GB; shadow.rs card_limits): forcing 1536 MB on a 12 GB
    # card can fill its memory.
    if ($RX7900XTX) {
        $settings.VICTORY_CARD_BUFFERS_MIN_MB = '1'; $settings.VICTORY_CARD_BUFFERS_MB = '1536'
    } else {
        $machine += ', card buffers by VRAM'
    }
    # From Op-lite's fork: pool pages given back when free RAM is low, the
    # title's fills not waiting behind flights, and only the changed levels of
    # a texture sent again (free roam: walking's slowest tenth 7.7 -> 14.1 fps).
    $settings.VICTORY_POOL_DISCARD = '1'; $settings.VICTORY_FILL_TITLE = '1'
    $settings.VICTORY_PARTIAL_DELTA = '1'
    # Renderer ports 1 and 5: the title's 32-bit graphics heap (its 174 MB
    # ring of constants) read and written by the card where the title has it,
    # and constant buffers read in place as raw bytes. The submitting thread
    # worked 60-63 ms a frame against 76-78 (jail, standing, two runs). Port 5
    # changes the translation: the first run after it starts from a cold
    # shader cache (`victory shaders`, then `victory warm --used --jobs 4`).
    $settings.VICTORY_PORT_CONSTANTS = '1'; $settings.VICTORY_PORT_CBV = '1'
    # Port 1 also for title buffers above 4 GiB that lie in one stretch of a
    # section (about 80 small ones, 15 MB): the submitting thread's snapshots
    # 4.5 -> 2.6 ms a frame and 10 MB a frame less compared on the CPU (jail,
    # standing, four runs: two with, two without).
    $settings.VICTORY_PORT_BUFFERS = '1'
    # Renderer port 2: the title's six descriptor record stores tell which slots
    # of its view heap they wrote, in place of asking the write watch for the
    # whole heap every round (a fallback looks at 16 of its written pages a frame).
    # In-run A/B (jail, standing, 15 phases of 20 s): the heaps' records 27.9 ->
    # 19.3 ms a frame, the submitting thread worked 55.2 -> 50.9 ms, frames 1.5%
    # less (ON below its OFF neighbours in 6 of 7). Of the records the fallback
    # finds untold, 98% name the view they named before; about 6 a frame are new
    # views, which no write of the CPU to the heap's addresses makes (a writer
    # trace caught only the six stores). No picture defect standing or walking.
    $settings.VICTORY_PORT_DESCRIPTORS = '1'
    # The views of the title's batch page-mapping copies (its GPU view of the streaming pool,
    # which its CPU does not touch) kept in our books and mapped only when touched, in place of
    # a map and an unmap call a page under the address-space lock. Two drives from the same save
    # (one exe, off and on): remapping 316 -> 118 ms a second (median), frames p90 177 -> 141 ms
    # and p99 457 -> 271 ms, time in frames of 250 ms or more 13.3 -> 5.4%, the median the same,
    # with more draws and less free RAM in the second; about 2,100 calls a second spared, three
    # views made real in the whole drive; no picture defect from it.
    $settings.VICTORY_SOFT_VIEWS = 'copies'
    # Host memory: preludes come from one pool shared by all lists, handed back
    # when their submission's fence is passed, in place of one command allocator
    # (about 1.5 MB, never given back) for each list. Jail, standing, two pairs
    # of runs: own preludes 1050 -> 0 (the pool made 74 and handed them out
    # 356251 times), write-combined memory 13.93 -> 12.43 GB with the same
    # textures alive (the other pair held 1 GB more textures and showed no
    # total drop). In-run A/B (15 phases of 20 s): +1.6% frame time, within
    # the 2% kept.
    # VICTORY_LIST_POOL_IDLE_S stays 0 (idle host lists kept): at 10 s most of
    # the lists it let go were made again on the title's threads within minutes
    # (194 let go, 169 made again; 313 and 259 in the other pair), for about 25
    # fewer lists alive.
    $settings.VICTORY_PRELUDE_POOL = '1'
    # The heaps' descriptor records read where they are and compared with AVX2
    # as read, in place of a copy of each changed page first (from zeex64's
    # fork). In-run A/B (19 pairs of 20 s phases: jail, cutscenes, swimming,
    # standing in the world): reading and comparing 4.6 -> 3.3 ms a frame,
    # lower in every pair; the submitting thread's wait for the pass 3.0 ->
    # 2.3 ms; frame times within the phases' noise; no picture defect.
    $settings.VICTORY_SYNC_IN_PLACE = '1'
    # Renderer port #6: the guards' lock shared, each block changed under its
    # own claim, so the title's write faults and the refreshes' looks at view
    # memory no longer queue behind one another (with it, the submission's
    # look at what copies nothing is planned beside the submitting thread).
    # In-run A/B on this preset (jail, standing, 15 phases of 20 s): all
    # threads' waits for the guards' lock 26.4 -> 4.0 ms a frame, finding
    # what view memory was written 16.3 -> 8.4 ms, protection changes 4.0 ->
    # 5.0 ms, frames 2.6% less (median against the OFF neighbours; ON below
    # them in 6 of 7). Measured before ports 1, 2 and 5 were on it was worse
    # (protection changes 7 -> 16 ms).
    $settings.VICTORY_PORT_GUARD = '1'
    # What shaders write to write-combined title memory (the title's CPU writes
    # it and does not read it back), in buffers of 1 MiB and more, is not
    # fetched back from the card (port from fxceless #868 + #1268 via
    # PastiiJay/klbf-lab). In-run A/B (jail, standing, 7 pairs of 20 s
    # phases): median frame 69.2 -> 51.9 ms, -17.0 ms, lower in every pair
    # (the heap's pass -6.9 ms, compares -1.25); moving, lower in the 2 pairs
    # with alike draws. A 5-minute walk and drive with it on throughout: no
    # picture defect, memory as in other drives. (Measured on main 44718da0,
    # before ports 6 and SOFT_VIEWS were on in this preset.)
    $settings.VICTORY_SKIP_WC_BACK = '1'
    # On a processor with two or more last-level caches of one size (a Ryzen
    # with two core complexes), our own threads are placed by core complex
    # (adapted from motioncsgo's patch in avatarmanz/VIctory-7900XTX issue
    # 399): near keeps the submitting thread and its helpers beside the
    # title's threads, on the cache with processor 0 where the title pins
    # them, and sends only the list translators to the other complex; far
    # sends all of ours there. In-run A/B on a Ryzen 9 5900X, median against
    # the OFF neighbours: far against none (jail, standing, 18 pairs of 20 s
    # phases, main 1c479b29): frames -3.3%, p90 -5.0%, lower in 17 of 18.
    # Near against far (main daef5bd3, this preset): standing in the jail, 12
    # pairs: -1.5%, p90 -6.6%, lower in 10 of 12; driving, 5 pairs: -14.8%,
    # p90 -13.7%, lower in 4 of 5 (the title's waits at presents -35%, list
    # translation -12%; the submitting thread works 11-18% more beside the
    # title). Nothing changes on one complex, X3D or two kinds of cores.
    $settings.VICTORY_CCD = 'near'
}
foreach ($k in $More.Keys) { $settings[$k] = $More[$k] }

# What this start runs with, one line, so a log or a terminal shows which preset it was.
$names = @(); if ($RX7900XTX) { $names += 'RX7900XTX' } elseif ($Tuned) { $names += "Tuned for $machine" }; if ($Safe) { $names += 'Safe' }; if ($RayTracing) { $names += 'RayTracing' }
if ($names.Count -eq 0) { $names = @('base') }
$line = ($settings.Keys | Sort-Object | ForEach-Object { "$($_ -replace '^VICTORY_', '')=$($settings[$_])" }) -join ' '
Write-Host "play.ps1 [$($names -join ' + ')] $Build, $($settings.Count) settings: $line"
if ($List) { return }

$victory = Join-Path $PSScriptRoot "..\target\$Build\victory.exe"
if (-not (Test-Path $victory)) { Write-Host "No $victory. Build it first (README.md)."; return }
if (-not (Test-Path $Exe)) { Write-Host "No $Exe."; return }
# (Other settings left over in this window go; where your files are stays.)
$paths = 'VICTORY_CONFIG', 'VICTORY_GAME_DIR', 'VICTORY_CACHE_DIR', 'VICTORY_DATA_DIR'
Get-ChildItem Env:VICTORY_* | Where-Object { $_.Name -notin $paths } | ForEach-Object { Remove-Item "Env:$($_.Name)" }
foreach ($k in $settings.Keys) { Set-Item "Env:$k" $settings[$k] }
$Exe = (Resolve-Path $Exe).Path
Push-Location (Join-Path $PSScriptRoot '..')   # victory.toml is looked for here
try {
    # The caches made ahead, with the same settings as the run (a translation is
    # served only under the key it was made with): shaders when the key changed,
    # then the pipelines earlier runs drew with that the driver does not have yet.
    if ($Warm -ne 'off') {
        $folder = Split-Path $Exe -Parent
        Write-Host "play.ps1: caches ahead (-Warm $Warm; -Warm off skips this)"
        if ($Warm -eq 'always') { & $victory shaders $folder } else { & $victory shaders --if-new $folder }
        & $victory warm --used
    }
    & $victory run $Exe --execute
} finally { Pop-Location }
