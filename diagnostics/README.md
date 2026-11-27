# Diagnostics

Tools that measure the host and read what the runtime's counters write. They never change the runtime.
Every script is read only and takes its files as arguments: nothing here knows where your logs, cache or
game are. Run Python scripts with `python -I <script> ...` (Python 3.8 or newer, standard library only);
each one's first lines say its arguments.

The counters themselves are runtime switches, off by default (see the top-level README):
`VICTORY_CPU_STATS`, `VICTORY_DISK_STATS`, `VICTORY_MEM_OWNERS`, `VICTORY_CHANGED_BYTES`, `VICTORY_AB`,
`VICTORY_TIMELINE` and the others named below.

The same build varies a lot from run to run: compare small changes inside one run with `VICTORY_AB`, not across runs.
Log times are UTC; window arguments are `HH:MM:SS` in UTC.

## logs/ — run logs

| Script | What it gives |
|---|---|
| `frames.py` | table of the frame-stats blocks of a log (fps, ms, preparing, copies, textures, draws) |
| `window_frames.py` | frame blocks between two times (a walk, a stand): count, median, p90, max ms/frame |
| `window_summary.py` | one window of a run: submitting thread's work, title's wait at presents, card memory, working set; optionally the medians of a `counters.ps1` CSV |
| `start_phase.py` | from entering the world, how long until frames settle, and the texture MB uploaded meanwhile |
| `frame_model.py` | where the frame goes: medians of the steady heavy blocks |
| `ab_logs.py` | two run logs of the same scene side by side |
| `ab_core.py` | a `VICTORY_AB` run with one switch: phase medians per side, each ON phase against its OFF neighbours, counters per phase |
| `ab2.py` | the same with two switches (bits 0 and 1), per combination |
| `cpustats.py` | `VICTORY_CPU_STATS` lines summed over the last reports |
| `changed.py` | `VICTORY_CHANGED_BYTES` lines (diagnostics build) summed |
| `intent.py` | what the title asked for (memory, resources), grouped, from a diagnostics log |
| `line_stats.py` | means per window of the `VICTORY_ASYNC_SUBMIT` line and the frame lines |
| `windows_stats.py` | means per window of the heap-records and `VICTORY_CPU_STATS` lines |
| `range_window.py` | per-range refresh lines over a window, the ten costliest ranges |
| `disk_watch.py` | follows the newest `run-*.log` in a folder while the game runs: the `VICTORY_DISK_STATS` report once a minute |

## memory/ — the process's memory from outside

| Script | What it gives |
|---|---|
| `memread.py` | attaches to `victory.exe` (no rebuild, read only, below-normal priority) and splits its commit into the title's reservations, the title's committed memory, the title's physical pages, write-combined and private memory of ours, each with how much is resident; plus the system commit over time |
| `growthwatch.py` | follows a run's log (and `memread.py`'s `.jsonl`) and prints one line per event: moving starts, status every few minutes, card memory growth stopping, texture copies let go, the game closing; `--replay` runs it over a finished log |

## pipelines/ — the pipeline book in `<cache>/pipelines`

| Script | What it gives |
|---|---|
| `book_stages.py` | pipelines by stage combination, which ones runs drew with (`early-pipelines.txt`), those with a geometry, hull or domain stage, and with `--watch` those whose shader containers carry given names |
| `book_trace.py` | the pipelines holding a shader whose container names include a given substring (or given shader hashes), and whether runs drew with them |
| `book_builder_nums.py` | the pipeline numbers of the ray-tracing structure builder, for `victory warm --these` |

## samples/ — CSVs of `probes/sampler`

| Script | What it gives |
|---|---|
| `threadprof.py` | for chosen threads: where each sample's wall time goes |
| `tracking_split.py` | samples whose leaf is page tracking (`GetWriteWatch`, `VirtualProtect`, `VirtualQuery`, ...), by caller |
| `who_waits.py` | per thread, wait samples whose stack goes through a substring |
| `lock_callers.py` | a thread's samples grouped by the first frames of ours above the leaf |
| `during_wait.py` | while thread A's stack holds a needle, what thread B is doing |
| `refresh_leaves.py` | a thread's samples through given functions, by leaf and by caller, busy and waiting apart |
| `raw_mutex_stacks.py`, `stack_peek.py` | a few raw stacks of wait samples |

## traces/ — `VICTORY_TIMELINE` traces (`trace.json`, summaries `.txt`)

| Script | What it gives |
|---|---|
| `heap_waits.py` | each wait of the submitting thread for the heaps' records, and what the heap-sync thread did meanwhile |
| `inside_spans.py` | what lies inside spans of a name on one thread: nested spans, waits, kernel calls, uncovered time |
| `one_flush.py` | inside one span of the submitting thread, what every thread did |
| `span_sizes.py` | the durations of one span name on one thread |
| `lock_wakers.py` | for waits on one lock, who woke them and in what span |
| `wait_objects.py` | for lock waits at a place, the locks waited on |
| `records_waits.py` | where the records threads and the heap-sync thread wait, and who woke them |
| `tl_sections.py` | chosen sections of timeline summaries |

## system/ — the host

| Tool | What it gives |
|---|---|
| `counters.ps1` | CPU, memory, page faults and GPU engine counters of `victory.exe` to CSV (`-Out <dir>`, default `.\counters`) |
| `ramtrend.py` | the game's memory minute by minute from `counters.ps1` CSVs |
| `clocks.py` | the card's clocks and sensors through AMD's ADL library, to screen or CSV |
| `trackbench.py` | micro-benchmark in its own process: the cost of `GetWriteWatch`, `VirtualProtect`, `VirtualQuery` |
| `crashdbg.py` | runs a program of ours under a tiny debugger until it crashes, and prints where |

## probes/ — Rust programs (build each with `cargo build --release` in its folder)

| Probe | What it measures |
|---|---|
| `sampler` | samples a running process's threads (optionally with stacks) to CSV; `ramap` maps its memory by category, `membench` and `busyloop` are small benchmarks |
| `gpuburst` | how the card's clock and the time of a fixed piece of work depend on gaps between submissions (AMD: reads clocks through ADL) |
| `uvmprobe` | what Windows D3D12 offers toward unified memory: reserved (tiled) buffers, tiles at two addresses, remaps |
| `wcprobe` | which D3D12 objects make the driver hold write-combined memory, phase by phase |
| `d3dprobe` | what the driver keeps in write-combined memory for command lists and upload buffers, one scenario per run |

`target/`, `__pycache__/` and `out/` folders are not kept here.
