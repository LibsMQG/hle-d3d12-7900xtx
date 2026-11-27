# Tuning: which preset, and what each setting is worth

This page is for people tuning their own PC, and for an AI assistant (Claude or another) asked
"which settings should I use?". It lists every setting the presets in
[`scripts/play.ps1`](../scripts/play.ps1) turn on, what each one does, what it measured, and
whether it is worth trying the other way. The numbers come from the runs behind this tree's
commits; the logs of the main ones are in [run-logs/](../run-logs/README.md).

**Where the numbers come from.** One PC: RX 7900 XTX 24 GB, Ryzen 9 5900X (12 cores, 24
threads, two core complexes), 32 GB of RAM, Windows 11 Pro. Most are in-run A/Bs
(`VICTORY_AB`, below): the switch turned on and off every 20 s inside one run, standing still in
one fixed test spot unless the row says driving. "Frames -3.3 %" means the median frame time
of the phases with the switch on, against the neighbouring phases with it off. The same build
varies by about 15 ms from run to run, so numbers from separate runs are weaker and say so.

## For an assistant: how to recommend

1. Ask for the PC: CPU model (logical processors, one or two core complexes, X3D or not, two
   kinds of cores or not), RAM in GB, graphics card and its VRAM, and what matters most to
   them (standing frame rate, driving, memory use, or just a picture without defects).
2. Pick the preset:
   - a PC like the reference one (24 GB AMD card, 12 or more cores in two core complexes,
     32 GB of RAM): `-RX7900XTX`, the measured set;
   - any other PC: `-Tuned`. It sizes threads, memory kept in place and card buffers to the
     PC; only the reference PC's sizes were measured, the others follow the same rule.
   - something wrong (crash, device lost, picture defects): `-Safe` first, then the
     [troubleshooting](#when-something-is-wrong) rows.
3. Tell them to run `scripts\play.ps1 -Tuned -List -Exe <their exe>` and read the first line:
   it says what the preset was sized for and every value it will start with.
4. Name the few settings that matter most on their PC (the tables below say which), and that
   the way to know is an in-run A/B on their own PC, not a guess: numbers here are of one PC.
5. Say plainly what is not measured: `-Tuned` as a whole on any PC, every NVIDIA and Intel
   card, CPUs below 16 threads, cards below 24 GB.

## Presets

| Switch of `play.ps1` | What it is |
|---|---|
| (none) | The base line below: the inherited settings every preset starts from. |
| `-RX7900XTX` (old name `-RX7900XT`) | `-Tuned` with the exact values measured on the reference PC, whatever PC runs it. Every number in the README is of it. |
| `-Tuned` | The same switches, the PC-dependent parts sized to the PC at start. Not tested as such on any PC; a starting point. |
| `-Safe` | Without the newer, faster ways: one part per joined submission, no list thinning, no uploads ahead, page compares on the submitting thread, no landing echo; light frames left at the title's 30. The first thing to try when something is wrong. |
| `-RayTracing` | The title's ray-tracing structure builder on (`VICTORY_BVH_BUILDER=1`): lit interiors, about a third slower; not yet measured on this tree. |
| `-Fps <n>` / `-Fps off` | Light frames (at most 500 draws: videos, loading, pause, map) at up to n a second; 60 by default, `off` leaves the title's 30. |
| `-SkipMovies once` / `all` | Skips the opening movies; `all` also the looping menu backgrounds (they stay dark). |
| `-Threads <n>`, `-RamGB <n>` | With `-Tuned`: size it as for another machine. |
| `-More @{NAME='value'}` | Any setting by hand, on top of the preset. |
| `-Warm auto` / `always` / `off` | Caches made ahead before the title starts, with the run's settings (README, "Caches made ahead"): `auto` (default) translates every shader only when the cache key changed and makes the pipelines earlier runs drew with that the driver lacks; `always` translates again; `off` starts at once. Scripted measurements may want `off` so a start is not held up. |
| `-List` | Prints the settings and stops. |

## Settings in `-Tuned` / `-RX7900XTX`, by weight

Ordered by what they measured on the reference PC. "Try off?" says whether turning it off is
worth an A/B on another PC.

| Setting | Value | What it does | Measured (reference PC) | Try off? |
|---|---|---|---|---|
| `VICTORY_SKIP_WC_BACK` | `1` | What shaders write to the title's write-combined memory (written by its CPU, never read back) is not fetched back from the card, for buffers of 1 MiB and more. | In-run A/B, 12 pairs: frames 70.1 -> 54.4 ms (-22.9 % median), submitting thread 55.2 -> 35.8 ms; drives without errors or defects. **The biggest single gain.** | Only for picture defects (dark rooms were the small buffers, already left out). |
| `VICTORY_CCD` | `near` | On a CPU with two core complexes, our submitting thread and helpers stay beside the title's threads; only the list translators go to the other complex. `far` sends all of ours away. | far against none: -3.3 % (17 of 18 pairs). near against far: -1.5 % standing (p90 -6.6 %), **-14.8 % driving** (p90 -13.7 %, the title's waits at presents -35 %). | No effect on one complex, X3D or two kinds of cores. On other two-complex CPUs, A/B `near` against `far`. |
| Renderer ports 1 and 5: `VICTORY_PORT_CONSTANTS`, `VICTORY_PORT_CBV` | `1` | The title's 174 MB ring of constants read and written by the card where it is; constant buffers read in place. | Submitting thread 76-78 -> 60-63 ms a frame (two runs each). Port 5 starts the first run from a cold shader cache. | No. |
| `VICTORY_PORT_JOURNAL` (port 7) | `1` | A round of the heaps' records asked for at the title's submit, so it runs while the submission waits in the line; the journal's notes in parts with locks of their own. | In-run A/B with eight list translators, 10 pairs standing: the submitting thread's wait for the heaps' records 6.6 -> 2.4 ms a frame, frames -6.2 % median (8 of 9 pairs lower, one even), p90 -1.8 ms. With four translators it was neutral (the submitting thread waited for translation then). One drive with the same A/B: the wait 5.9 -> 3.0 ms, frames within the drive's noise, no error or new defect. | Yes with fewer list translators: it pays only where the submitting thread sets the frame. |
| `VICTORY_PORT_GUARD` (port 6) | `1` | The guards' lock shared, each block changed under its own claim. | In-run A/B: lock waits 26.4 -> 4.0 ms a frame, frames -2.6 % (6 of 7). Cleared of the house-texture doubt by a later A/B. | No. |
| `VICTORY_PORT_DESCRIPTORS` (port 2) | `1` | The title's descriptor stores tell which slots they wrote, instead of the whole view heap being asked every round. | In-run A/B: heap records 27.9 -> 19.3 ms, submitting thread 55.2 -> 50.9 ms, frames -1.5 % (6 of 7). | No. |
| `VICTORY_SOFT_VIEWS` | `copies` | The views of the title's batch page-mapping copies kept in our books and mapped only when touched. | Two drives: remapping 316 -> 118 ms a second, p99 457 -> 271 ms, time in 250 ms+ frames 13.3 -> 5.4 %; median the same. **Driving only.** | Only for picture defects (`off`). |
| Helper threads `VICTORY_THIN_THREADS` / `UNDER_THREADS` / `LAND_THREADS` | 8 / 3 / 3 (`-Tuned`: by logical processors, 8/3/3 from 24, 4/3/3 from 16) | Our threads that translate list calls, compare title bytes and land copies. | On 24 threads with `VICTORY_CCD=near` (in-run A/B, jail, standing, two runs): eight translators against four, frames -0.8% and -3.4% median, p90 -7% and -9%, the submitting thread's wait for untranslated lists 24-26 -> 11 ms a frame, a third more of the translators' work. On 16 threads 8/6 were too many: free roam 6-13 ms less preparing a frame with 4/3/3. Smaller counts for fewer processors are not measured. | **Yes on CPUs under 24 threads, or with one core complex**: one step up or down, A/B by runs. |
| `VICTORY_PORT_BUFFERS` | `1` | Port 1 also for title buffers above 4 GiB lying in one stretch (about 80, 15 MB). | Snapshots 4.5 -> 2.6 ms a frame, 10 MB a frame less compared (four runs). | No. |
| `VICTORY_SYNC_IN_PLACE` | `1` | Heap records read where they are and compared with AVX2, no copy of each page first. | In-run A/B, 19 pairs: reading and comparing 4.6 -> 3.3 ms, lower in every pair; frames within noise. | No. |
| `VICTORY_POOL_DISCARD`, `VICTORY_FILL_TITLE`, `VICTORY_PARTIAL_DELTA` | `1` | Pool pages given back when free RAM is low; the title's fills not waiting behind flights; only changed texture levels sent again. | Free roam, walking's slowest tenth 7.7 -> 14.1 fps (Op-lite's fork, older line). | No. |
| `VICTORY_PRELUDE_POOL` | `1` | One shared pool of preludes instead of one command allocator (~1.5 MB, never freed) per list. | Write-combined memory 13.93 -> 12.43 GB (-1.5 GB); frames +1.6 % (within the 2 % kept). **Memory, not speed.** | Only if frames matter more than 1.5 GB (`0`). |
| `VICTORY_ASK_EARLY`, `VICTORY_HEAP_SYNC_EARLY`, `VICTORY_JOIN_PARTS`, `VICTORY_JOIN_LISTS`, `VICTORY_TITLE_PRIORITIES` | `2`, `1`, `16`, `96`, `1` | Write watch asked early, heap records every 1 ms, longer joined submissions, the title's idle threads below normal. | Measured on an older line for 20 fps on a 7800X3D (CHANGELOG.md); not re-measured here one by one. | `JOIN_PARTS` is A/B-able; the rest by runs. |
| `VICTORY_INPLACE`, `VICTORY_INPLACE_MB` | `1`, `3072` (`-Tuned`: 3072 / 1536 / 768 by RAM) | Windows of title memory in private memory used in place; up to this much locked into RAM. | 3072 measured with 32 GB; the smaller sizes are not measured. | **Yes with 16 GB of RAM or less**: lower it if the PC pages. |
| `VICTORY_CARD_BUFFERS_MIN_MB`, `VICTORY_CARD_BUFFERS_MB` | `1`, `1536` (`-RX7900XTX` only) | Copies of title buffers in VRAM, for shader-write compares on the card. `-Tuned` leaves it to the build: 16 MiB / 512 MB up to 12 GB, 4 MiB / 2048 MB on 16 GB, 1 MiB / up to 8192 MB from 20 GB. | 1536 MB is the measured value; the build's sizes are not measured. | **Yes with 12 GB of VRAM or less**: watch "Dedicated GPU memory" (the title uses ~11 GB). |
| `VICTORY_ROOT_WINDOW_MB`, `VICTORY_WINDOW_KB`, `VICTORY_TEXTURE_STAGE_MS` | `12`, `1024`, `80` | Smaller windows of title memory (about 5 GB less committed), smaller texture steps at scene cuts. | Memory, from older lines. | No. |
| `VICTORY_PIPELINE_BOOK` | `1` | The pipelines the title creates are written down, for `victory warm`. | Tool for warming the cache. | No. |

## Settings in every preset (the base line)

Inherited from the base tree and the forks before it; kept because the picture was complete
with them. Not re-measured one by one on this tree.

| Setting | Value | What it does |
|---|---|---|
| `VICTORY_FPS` | `60` | Light frames at up to 60 a second (the title's own limiter let through, its shortest simulation step following). Loading 30 -> 59 fps, pause 51-60, map 30 -> 42-50, game time 989-1012 ms a second; the world unchanged. |
| `VICTORY_RELAY` | `2` | Card-side copies relayed back in two stages (walls of the first room drawn in every run with it). **`0` if a DXGI device reset appears.** |
| `VICTORY_TEXTURE_NO_WAIT`, `VICTORY_RELAY_COPY`, `VICTORY_JOIN_COPIES`, `VICTORY_CARD_REHASH_RUNS`, `VICTORY_CARD_STEPS=2` | `1` | Texture fills and copies without extra waits; joined copies; landings told to the guards. |
| `VICTORY_INDIRECT_WINDOW`, `VICTORY_INDIRECT_BATCH=2` | `1` | Indirect draw arguments read through a window and batched. |
| `VICTORY_WIDE_BACK`, `VICTORY_MAP_THREADS` | `1` | Wide fetch-backs; page mapping on one thread. |
| `VICTORY_SHOW_AFTER` | `1` | Textures with page faults in the last tenth of a second shown a frame later. |
| `VICTORY_AMBIENT` | `0.8` | Estimate of the indirect light while ray tracing is off. |
| `VICTORY_THIN`, `VICTORY_JOIN_PARTS=8`, `VICTORY_UPLOAD_AHEAD` | `1` | List thinning, joined submissions, textures uploaded ahead of the lists (off with `-Safe`). |
| `VICTORY_HOST_EARLY` | on (build default) | The host GPU device made before the title starts, so its 30 s start-up watchdog does not end the run while AMD's driver reads its cache. |

## Measured, and left off

| Setting | Measured | Why off |
|---|---|---|
| `VICTORY_CARD_AWAKE` | Card at 100 % all the time, about 100 W more. | No frame gain worth it; `dynamic` (awake only while the submitting thread waits) is being tried. |
| `VICTORY_BUFFER_SKIP_WAIT`, `VICTORY_SYNC_FINE` | Frames -1.4 % and -1.6 %, inside the noise. | The resident waits are not on the submitting thread and guard against device losses; `SYNC_FINE` to try in the city. |
| `VICTORY_LANE_CORE` | +0.4 % on top of `VICTORY_CCD` (5 of 12 pairs). | No gain. |
| `VICTORY_LIST_POOL_IDLE_S` | At 10 s most lists let go were made again within minutes. | Churn. |
| `VICTORY_TITLE_FPS` | The whole title at N, the world too. | Off by default: the world at the title's own rate. |
| `VICTORY_BVH_BUILDER` (`-RayTracing`) | About a third slower, lit interiors. | Not measured on this tree yet (issue 2). |

Being ported from [eikkapine/victory-amd](https://github.com/eikkapine/victory-amd), off until
measured: driving pipeline waits and texture queue order, the tree flicker fix, light without
ray tracing, mouse and keyboard, FSR and frame generation (README, [Collaboration](../README.md#collaboration)).

## When something is wrong

| Symptom | Try |
|---|---|
| Anything | `-Safe`, then add back one thing at a time with `-More`. |
| DXGI device reset / device lost | `-More @{VICTORY_RELAY='0'}`. |
| The title ends at start-up after about 30 s | Keep `VICTORY_HOST_EARLY` on (default); let the driver's shader cache build once. |
| Card memory full (12 GB or less) | Use `-Tuned`, not `-RX7900XTX`; lower `VICTORY_CARD_BUFFERS_MB`. |
| The PC pages (16 GB of RAM or less) | Lower `VICTORY_INPLACE_MB`; keep `VICTORY_PRELUDE_POOL` on. |
| Dark rooms, missing walls | `VICTORY_SKIP_WC_BACK=0`, `VICTORY_SOFT_VIEWS=off`, one at a time. |
| Long frames while driving | Expected: pipelines met for the first time (README, Known limits). `-Warm auto` (default) makes the ones earlier runs drew with before the next start, so a second drive through the same places has fewer. |
| Missing textures for minutes after an update | The shader cache key changed: `-Warm auto` translates everything before the start; check that `victory shaders` and the run's log print the same "shader cache key" line. |

## How to measure on your own PC

Use a diagnostics build (`cargo build --locked --profile diag -p victory --features diagnostics`)
and an in-run A/B: `-More @{VICTORY_AB='20,VICTORY_SKIP_WC_BACK'}` turns the named switch on and
off every 20 s while you stand still; `python -I diagnostics/logs/ab_core.py <log> <first phase>
<last phase>` gives the median of each side and each ON phase against its OFF neighbours (leave
out the phases before you stood still). Switches that can change while the title runs:
`VICTORY_CCD`, `VICTORY_JOIN_PARTS`, `VICTORY_PORT_DESCRIPTORS`, `VICTORY_PORT_GUARD`,
`VICTORY_PORT_JOURNAL`, `VICTORY_PRELUDE_POOL`, `VICTORY_SKIP_WC_BACK`, `VICTORY_SYNC_IN_PLACE`,
`VICTORY_UPLOAD_AHEAD` and a few more (`crates/victory-gpu/src/ab.rs` and its callers). Any other
setting: two runs each way, and treat differences under ~15 ms as noise.
