# Diagnostics

Tools that read what the runtime logs and measure the host. They never change the runtime.
Python scripts are read only: run them with `python -I <script> ...`. Each script's first lines say what it reads and how to call it.

## logs/ — run logs

They read the counters the runtime writes when its switches are on (`VICTORY_CPU_STATS`, `VICTORY_CHANGED_BYTES`,
`VICTORY_ASYNC_SUBMIT`, `VICTORY_DISK_STATS`, `VICTORY_AB`, `VICTORY_LOG=...=debug`) and its frame lines.

| Script | What it gives |
|---|---|
| `cpustats.py` | `VICTORY_CPU_STATS` summed over the last reports, weighted by frames |
| `changed.py` | `VICTORY_CHANGED_BYTES`: bytes really changed, per reservation |
| `intent.py` | what the guest asked for (memory, resources), grouped; needs the xmem/resource debug log |
| `frames.py`, `frame_model.py` | per-block frame table; medians of where the frame goes |
| `window_frames.py` | frame blocks and disk reports between two UTC times |
| `line_stats.py`, `windows_stats.py`, `range_window.py` | means of the submit, heap-records, CPU and refresh lines per UTC window |
| `start_phase.py` | how long a run takes to settle after the first heavy frames |
| `ab_logs.py` | two logs of the same scene, side by side |
| `ab_core.py`, `ab2.py` | in-run A/B (`VICTORY_AB`) with one or two switches: each ON phase against its OFF neighbours |

The same build varies a lot from run to run: compare small changes inside one run with `VICTORY_AB`, not across runs.

## traces/ — timeline traces (`trace.json`)

Waits, who woke whom, and what lies inside spans: `heap_waits.py`, `inside_spans.py`, `lock_wakers.py`,
`one_flush.py`, `records_waits.py`, `span_sizes.py`, `wait_objects.py`. `tl_sections.py` prints chosen sections of a timeline summary.

## samples/ — sampler CSVs (made by `probes/sampler`)

Where a thread's wall time goes and what it waits on: `threadprof.py`, `tracking_split.py` (page tracking calls),
`who_waits.py`, `lock_callers.py`, `during_wait.py`, `refresh_leaves.py`, `raw_mutex_stacks.py`, `stack_peek.py`.

## system/ — the host

| Tool | What it gives |
|---|---|
| `counters.ps1` | CPU, memory, page faults and GPU engine counters of `victory.exe` to CSV (`-Out <dir>`, default `.\counters`) |
| `ramtrend.py` | the process's memory minute by minute, from those CSVs |
| `trackbench.py` | cost of `GetWriteWatch` and `VirtualProtect` per 64 KiB block (own process only) |
| `crashdbg.py` | runs a program until it crashes and prints where (Windows debug API, only the process it starts) |
| `clocks.py` | GPU clocks and sensors through AMD's ADL library (AMD cards only, read only) |

## probes/ — Rust programs (build each with `cargo build --release` in its folder)

| Probe | What it measures |
|---|---|
| `sampler` | samples a running process's threads (optionally with stacks) to CSV; `ramap` maps its memory by category, `membench` and `busyloop` are small benchmarks |
| `gpuburst` | how the card's clock and the time of a fixed piece of work depend on gaps between submissions (AMD: reads clocks through ADL) |
| `uvmprobe` | what Windows D3D12 offers toward unified memory: reserved (tiled) buffers, tiles at two addresses, remaps |
| `wcprobe` | which D3D12 objects make the driver hold write-combined memory, phase by phase |
| `d3dprobe` | what the driver keeps in write-combined memory for command lists and upload buffers, one scenario per run |

`target/` folders are not kept here.
