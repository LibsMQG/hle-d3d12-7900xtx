# Starts the title with the settings this fork is played with (README.md, "How to run it").
#   scripts\play.ps1 -Exe <path of your own copy's executable>
# -Safe         without the newer, faster ways (the first thing to try when something is wrong)
# -RayTracing   the title's own ray-traced indirect light: lit interiors, about a third slower
# -Build        target\diag (the default, the build all measurements are of) or target\release
# -RX7900XT     also the settings measured best on an RX 7900 XT with 32 GB of RAM (docs/rx7900xt.md)
# -More         other settings: -More @{VICTORY_PANEL='1'}
# Run it from a normal window, not as administrator: the driver then keeps its shader cache elsewhere.
param(
    [Parameter(Mandatory)][string]$Exe,
    [switch]$Safe,
    [switch]$RayTracing,
    [switch]$RX7900XT,
    [ValidateSet('diag', 'release')][string]$Build = 'diag',
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
if ($RX7900XT) {
    # Shader-write compares on the card for buffers of 1 MiB and more; less committed
    # memory in windows of title memory (about 5 GB less); smaller texture steps at scene
    # cuts; windows in private memory used in place; pipelines written down for "victory warm".
    $settings.VICTORY_CARD_BUFFERS_MIN_MB = '1'; $settings.VICTORY_CARD_BUFFERS_MB = '1536'
    $settings.VICTORY_ROOT_WINDOW_MB = '12'; $settings.VICTORY_WINDOW_KB = '1024'
    $settings.VICTORY_TEXTURE_STAGE_MS = '80'
    $settings.VICTORY_INPLACE = '1'; $settings.VICTORY_INPLACE_MB = '3072'
    $settings.VICTORY_PIPELINE_BOOK = '1'
    # Measured for 20 fps on the 7800X3D (CHANGELOG.md): the write watch asked
    # early, the heaps' records every 1 ms, longer joined submissions, and the
    # title's idle threads at below normal (16 threads are not enough for them
    # to wait their turn).
    $settings.VICTORY_ASK_EARLY = '2'; $settings.VICTORY_HEAP_SYNC_EARLY = '1'
    $settings.VICTORY_JOIN_PARTS = '16'; $settings.VICTORY_JOIN_LISTS = '96'
    $settings.VICTORY_TITLE_PRIORITIES = '1'
    # Fewer of our threads translating list calls and comparing title bytes
    # (8 and 6 were for a 24-thread CPU): on 16 the title's own threads ran
    # short (free roam: 6-13 ms less preparing a frame, CHANGELOG.md).
    $settings.VICTORY_THIN_THREADS = '4'; $settings.VICTORY_UNDER_THREADS = '3'
    $settings.VICTORY_LAND_THREADS = '3'
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
    # Host memory: preludes come from one pool shared by all lists, handed back
    # when their submission's fence is passed, in place of one command allocator
    # (about 1 MB, never given back) for each list; and idle host lists are let
    # go after 10 s. Jail, standing, two pairs of runs: own preludes 1050 -> 0
    # (the pool made 74 and handed them out 356251 times), 313 idle lists let go,
    # write-combined memory 13.93 -> 12.43 GB with the same textures alive (the
    # other pair held 1 GB more textures and showed no total drop). In-run A/B
    # (15 phases of 20 s): the pool +1.6% frame time, within the 2% kept; the
    # idle let-go -0.2%.
    $settings.VICTORY_PRELUDE_POOL = '1'
    $settings.VICTORY_LIST_POOL_IDLE_S = '10'
    # The heaps' descriptor records read where they are and compared with AVX2
    # as read, in place of a copy of each changed page first (from zeex64's
    # fork). In-run A/B (19 pairs of 20 s phases: jail, cutscenes, swimming,
    # standing in the world): reading and comparing 4.6 -> 3.3 ms a frame,
    # lower in every pair; the submitting thread's wait for the pass 3.0 ->
    # 2.3 ms; frame times within the phases' noise; no picture defect.
    $settings.VICTORY_SYNC_IN_PLACE = '1'
}
foreach ($k in $More.Keys) { $settings[$k] = $More[$k] }

$victory = Join-Path $PSScriptRoot "..\target\$Build\victory.exe"
if (-not (Test-Path $victory)) { Write-Host "No $victory. Build it first (README.md)."; return }
if (-not (Test-Path $Exe)) { Write-Host "No $Exe."; return }
# (Other settings left over in this window go; where your files are stays.)
$paths = 'VICTORY_CONFIG', 'VICTORY_GAME_DIR', 'VICTORY_CACHE_DIR', 'VICTORY_DATA_DIR'
Get-ChildItem Env:VICTORY_* | Where-Object { $_.Name -notin $paths } | ForEach-Object { Remove-Item "Env:$($_.Name)" }
foreach ($k in $settings.Keys) { Set-Item "Env:$k" $settings[$k] }
$Exe = (Resolve-Path $Exe).Path
Push-Location (Join-Path $PSScriptRoot '..')   # victory.toml is looked for here
try { & $victory run $Exe --execute } finally { Pop-Location }
