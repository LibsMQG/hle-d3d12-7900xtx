# Diagnostics

Tools that measure the host. They never change the runtime.
The scripts that read the runtime's logs, traces and sampler CSVs are not kept in this repository.

## system/ — the host

| Tool | What it gives |
|---|---|
| `counters.ps1` | CPU, memory, page faults and GPU engine counters of `victory.exe` to CSV (`-Out <dir>`, default `.\counters`) |

## probes/ — Rust programs (build each with `cargo build --release` in its folder)

| Probe | What it measures |
|---|---|
| `sampler` | samples a running process's threads (optionally with stacks) to CSV; `ramap` maps its memory by category, `membench` and `busyloop` are small benchmarks |
| `gpuburst` | how the card's clock and the time of a fixed piece of work depend on gaps between submissions (AMD: reads clocks through ADL) |
| `uvmprobe` | what Windows D3D12 offers toward unified memory: reserved (tiled) buffers, tiles at two addresses, remaps |
| `wcprobe` | which D3D12 objects make the driver hold write-combined memory, phase by phase |
| `d3dprobe` | what the driver keeps in write-combined memory for command lists and upload buffers, one scenario per run |

The same build varies a lot from run to run: compare small changes inside one run with `VICTORY_AB`, not across runs.

`target/` folders are not kept here.
