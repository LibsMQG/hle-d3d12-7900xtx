# Run logs

Excerpts of the runtime's own logs from measured runs, kept so that the open issues can point at
real numbers. Each file starts with a header: the build and switches, the scene, the window of the
run it covers (UTC) and which lines were kept.

The PC: AMD Radeon RX 7900 XTX 24 GB, AMD Ryzen 9 5900X, 32 GB RAM, Windows 11 Pro.

## How they were made

From the full logs (10-30 MB each), on the way in:

- only lines of the layer's own crates are kept (the GPU layer's counters, the memory module, a few
  runtime lines); the title's own output, its user and network calls, its file names and its
  modules are left out;
- paths, the PC's name, the title's id, name and build hash, its module and file names and console
  names are replaced by placeholders such as `<project>`, `<host>`, `<title id>`, `<title image>.dll`;
- lines are thinned to fit the repository's 640 KiB file limit: at most one line of each kind every
  few seconds (the header says how many); A/B phase lines, frame-time blocks and long frames are
  all kept (in the drives, long frames from 250 ms or more);
- the result is checked against a list of names that must not appear.

Timestamps are the log's own (UTC), so the scripts in [`diagnostics/logs`](../diagnostics/logs)
read these files as they read a full log, for example
`python -I diagnostics/logs/ab_core.py run-logs/submit-thread/2026-11-27T0846Z-port6-ab.txt 10 24`.
Thinning leaves fewer samples per window, so medians can differ a little from the full log's.

## What is here

| Folder | File | What it shows |
|---|---|---|
| `standing/` | `2026-11-28T0147Z-bench-release.txt` | The window behind the README's release snapshot (Milestones, 2026-11-28): main 48e6467b with port 7, `VICTORY_TEXTURE_TURN=32` and the windows in card memory, all from `-RX7900XTX`, release build, standing in the test spot. |
| `standing/` | `2026-11-27T2109Z-bench-port7.txt` | The window behind the README's "Results at a glance" (this fork): main 48e6467b with renderer port 7 on, `-RX7900XTX`, standing in the test spot. |
| `standing/` | `2026-11-27T0757Z-main-counters.txt` | The window behind the previous "Results at a glance" column (64.7 ms): main 1ef9e073 plus counters, standing in the test spot. |
| `standing/` | `2026-11-27T0828Z-base-a8f83a3.txt` | The same spot on the base tree (root commit a8f83a3): the "RYUKI base" column. |
| `standing/` | `2026-11-27T1219Z-ccd-near-ab.txt` | The newest standing run with everything of today's `-Tuned` on; in-run A/B of `VICTORY_CCD` near against far. |
| `submit-thread/` | `2026-11-27T0834Z-refresh-kinds.txt` | The submitting thread's refreshes split by what the write watch knew before asking. |
| `submit-thread/` | `2026-11-27T0907Z-refresh-copies.txt` | Why refreshed pages are taken again; about half is the echo of our own landings. |
| `submit-thread/` | `2026-11-27T0846Z-port6-ab.txt` | In-run A/B of renderer port 6 (`VICTORY_PORT_GUARD`): the guards' lock and protection changes. |
| `port2/` | `2026-11-27T0223Z-port2-sort.txt` | Renderer port 2 with the full fallback and the sorting counter: view records no store told of. |
| `port2/` | `2026-11-27T0257Z-port2-trace.txt` | The same with the hardware-breakpoint writer tracer. |
| `driving/` | `2026-11-27T0653Z-drive-soft-off.txt` | A drive with `VICTORY_SOFT_VIEWS=off`: remapping-led long frames. |
| `driving/` | `2026-11-27T0702Z-drive-soft-copies.txt` | The same drive with `VICTORY_SOFT_VIEWS=copies`: long frames are mostly pipeline waits. |
| `driving/` | `2026-11-27T1233Z-drive-ccd-near-ab.txt` | The newest drive with today's `-Tuned`; in-run A/B of `VICTORY_CCD` near against far. The `textures picked by index, the queue` lines (in all three drives) show the texture queue falling behind. |
| `memory/` | `2026-11-27T0716Z-memory-drive.txt` | Standing in a house, then about 3 minutes of driving: commit, card memory and texture copies grow. |
| `memory/` | `2026-11-27T0813Z-memory-idle-growth.txt` | Ten minutes in the open world with nobody at the controls: the same growth, no plateau. |
| `capture/` | `2026-11-27T1137Z-capture-watchdog.txt` | A frame capture (`VICTORY_RECORD`) that stalls the frame until the title's watchdog stops it; our crash report at the end. |

Counters named "diagnostics counters (unpublished branch refresh-count)" in a header come from a
diagnostics-only branch that is not in this repository yet; their lines are self-explanatory, and
the branch can be published if someone wants to rebuild them.
