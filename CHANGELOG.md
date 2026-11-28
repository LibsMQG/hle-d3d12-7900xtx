# Changelog of this fork

Each entry names the commit and, where there is one, a tag to go back to:
`git checkout <tag>` to look, `git reset --hard <tag>` on a branch to return
(after saving anything newer elsewhere). Frame rates are medians of the log's
frame lines in the world, diagnostics build, on the PC in the README.

## Less of the PC's RAM held in `-Tuned` (2026-11-28)

`play.ps1 -Tuned` and `-RX7900XTX` set `VICTORY_WINDOWS_BAR_PCT=28`,
`VICTORY_LIST_LAZY=1`, `VICTORY_POOL_DISCARD_FRESH=1`, `VICTORY_SHRINK_KEPT=1` and
`VICTORY_POOL_DISCARD_BELOW_MB=0` (each off by itself, docs/tuning.md). Release
drives on the reference PC from the same start: nothing keeps growing (the
game's private bytes rise only while the card fills, as the driver mirrors the
card in commit, and stop at the room rule's line); in the first minutes driving
free RAM 1.1-2.7 -> 2.6-3.9 GB and the game's working set 15.2-16.2 -> 13.3-15.5
GB. Frames were not compared: the second drive went through far heavier streets.
`VICTORY_VRAM_LINE_PCT` (the room rule's line, 88 as before) is new and off.

## Windows in card memory in `-Tuned`; placed views' shapes no longer grow (2026-11-28)

`play.ps1 -Tuned` and `-RX7900XTX` set `VICTORY_WINDOWS_BAR_MB=6144`: the
windows (1 MiB copies of title memory the card reads, 4-6 GB of the PC's
memory pinned while driving) go to card memory the CPU maps (resizable
BAR), up to 20 % of the card's budget and none under a budget of 10 GiB;
the room rule lets the longest idle texture copies go to make room for
them (1.4-2 % of those are made again). Drives on the reference PC: the
PC's memory pinned for the card 7.7-10.2 -> 4.0-5.3 GB; at the world's load
free RAM 0.9-2.0 -> 5.2 GB and the game in memory 16.7 -> 13.6 GB; frames
within the drives' noise; no picture defect. Memory, not speed.

The shapes of placed views, which grew for the run (1.9 million after eight
minutes of driving, 550 MB of our heap, never let go), grow far less: the
words the title leaves from its stack (the holes of a resource's description
and of a view's) are left out of them, and views of whole placed buffers
are named whole in their records by a model and their length. A drive made
about 8,000 shapes by the end of the load where it had made 113,000.
`VICTORY_PLACED_FILLER=1` keeps the words as before.

## Soft-view textures hashed on a longer turn in `-Tuned` (2026-11-28)

`play.ps1 -Tuned` and `-RX7900XTX` set `VICTORY_TEXTURE_TURN=32`: a guarded
texture all of whose memory lies in the views kept in our books
(`VICTORY_SOFT_VIEWS`), which the guard saw unwritten and whose hash never
found a change behind the guard, is hashed every 32nd frame instead of every
8th; hot textures and videos written through another view keep the short
turn. In-run A/B on the reference PC with port 7 (jail, standing, 11 pairs of
20 s phases, a counter of changed bytes on both sides): textures hashed 17.3
-> 5.5 MB a frame, the submitting thread's texture time -0.9 ms and its work
-1.4 ms a frame, frames -1.1 % median of the pairs (8 of 11 lower), -1.5 % at
equal draws, p90 61.0 -> 58.3 ms; no change the guard did not see and no
picture defect, a loading screen included. Driving not measured.

## Renderer port 7 on in `-Tuned` (2026-11-27)

`play.ps1 -Tuned` and `-RX7900XTX` turn on renderer port 7
(`VICTORY_PORT_JOURNAL=1`): a round of the heaps' records is asked for when
the title submits, so it runs while the submission waits in the line, and the
journal's notes are kept in parts with locks of their own. Standing in the
cell the submitting thread sets the frame: it works about 50 ms of a 53 ms
frame and waits for memory values the card writes the rest, so what is taken
off its work comes off the frame. In-run A/B on the reference PC with eight
list translators (jail, standing, 10 pairs of 20 s phases): its wait for the
heaps' records 6.6 -> 2.4 ms a frame, its work 41.4 -> 38.4 ms, frames 49.1 ->
46.1 ms median (-6.2 %; ON below its OFF neighbours in 8 of 9 pairs, one
even), p90 55.3 -> 53.5 ms; the long frames of the 35/65 ms turns 64.5 ->
59.9 ms; no picture defect. The same A/B with four translators was neutral:
the submitting thread then stood at lists still being translated. One drive
with the same in-run A/B (standing, then on foot and in the car): no error and
no new picture defect; over the run the wait for the heaps' records 5.9 -> 3.0
ms a frame and the submitting thread's work 65.9 -> 62.3 ms; the driving
phases' frames within the drive's noise (ON against its OFF neighbours: median
+1.2 %, mean -0.8 % over 7 phases of 4,000 to 11,600 draws).

## Caches made ahead by `play.ps1 -Warm` (2026-11-27)

Before each start `play.ps1` runs `victory shaders --if-new <the title's
folder>` and `victory warm --used` with the run's own settings (`-Warm auto`,
the default; `always`, `off`). A translation is served only under the key it
was made with, and that key holds the translator's settings (port 5 among
them): `victory shaders` started by hand without `-Tuned`'s settings fills the
cache under another key, and the run then translates everything again. Both now
print the key ("shader cache key ...": the host's wave width, view-heap null
slot, raw view twins, RDNA bits and port 5), the run's log too, and `--if-new`
reads nothing when the last complete pass had the same key. `victory warm`
starts at most four processes unless asked (sixteen left 1,912 of 2,432
pipelines without a host pipeline on a cold driver cache, four made all), no
longer writes down as made a pipeline left without one, and makes those once
more with one process alone at the end.

## Eight list translators on 24 threads (2026-11-27)

`play.ps1 -RX7900XTX`, and `-Tuned` on 24 logical processors and more, start
eight threads that translate list calls (`VICTORY_THIN_THREADS=8`) in place of
four. In-run A/B on the reference PC (Ryzen 9 5900X, `VICTORY_CCD=near`, so the
translators sit on the core complex without the title's threads), jail,
standing, two runs of 20 s phases: frames -0.8% and -3.4% (median against the
neighbours), p90 -7% and -9%. With four, the submitting thread stood 24-26 ms a
frame at lists not translated yet, and a list waited 26 ms on average after
the title closed it; with eight, 11 ms and 7 ms. The translators' work grows by
a third (more threads share the same structures). One list is still translated
by one thread at a time: the longest (some 1,900 calls) takes about 30 ms.

## `play.ps1 -Tuned` (2026-11-27)

The measured settings are started with `-Tuned`, in place of `-RX7900XT`: most
of them are about the 16-thread CPU and 32 GB of RAM, not the card. The old
name still works (an alias of the same switch). Each start now prints one line
with the preset and every setting it sets, so a log shows what a run had;
`-List` prints that line and stops. `-Safe -Tuned` keeps one part per joined
submission (`VICTORY_JOIN_PARTS`), as `-Safe` alone does. `-Tuned` without
`-Safe` sets the same 41 settings as before.

## What the repository carries, checked (2026-11-26)

Microsoft Defender (signatures of the day) finds nothing in the repository's
archive, its files or either build. No executables, archives or encoded data;
one script (`scripts\play.ps1`). The README no longer suggests starting
PowerShell with the execution policy bypassed (a phrase scanners and chat
filters flag); it shows Microsoft's per-user setting instead, and a line of it
that an edit had broken is mended. Op-lite's fork's `tools/` folder (their
working notes and patch files, among them notes on the game's own code) is not
carried any more: nothing builds from it.

What a scanner may still dislike in the program itself is what a compatibility
layer does: it loads another program's executable into its own process,
changes page protections, catches access faults and writes into its own memory
through the kernel. None of it touches another process.

## The purple in shadows fixed (2026-11-26)

Blinds, curtains, bushes and trees glowed purple, most at night. Bisected
on fixed saves (a copy of the user's, restored before each run) with the
share of purple pixels in 12 shots per run: none before cool_fox, none at its
first two commits, purple from `8cc31ab3` on; the same with cool_fox's
translator taken out (an older translator over its GPU side), so not the
shader changes. The cause: without the ray-tracing builder, cool_fox left out
the `CS_RaytracingResolve` and `CS_ScreenProbes_ActivateRays` pipelines (to
unclog a cold compiler queue). The screen probes' light goes through them;
without them the probes kept what their memory held, and the stand-in light
spread it into the shadows. They run again unless `VICTORY_RT_RESOLVE=0` /
`VICTORY_ACTIVATE_RAYS=0`. Purple pixels: 15-69 per mille in a shot before,
0-1 after. Tag `goal-20fps-step7`.

## Op-lite's fork merged (2026-11-26)

Op-lite's fork, its cool_fox-based branch: cool_fox (the same base) and
one commit of picture fixes (16-bit values through integer views: frozen
facial animation; a NaN guard on colour exports; 26 more compute programs
translated: hair shadow masks, water volumetrics, particles, decals), memory
and upload switches, frame logs and meters. Their scripts and Ghidra scripts
are left out (notes and patches kept). `play.ps1 -RX7900XT` sets their three
best switches: `VICTORY_POOL_DISCARD=1`, `VICTORY_FILL_TITLE=1`,
`VICTORY_PARTIAL_DELTA=1`. Translate the shaders again after this merge. Tag
`goal-20fps-step6`.

Free roam, diagnostics build, walking: 15.0 fps, slowest tenth 14.1 (12.5 and
7.7 without the three switches); about 2.5 GB less of the process in RAM.

Their notes say what was seen here too: bushes and trees glow purple, and it
came with the cool_fox base (fixed since: the entry above).

Measured for RAM and not kept: fewer idle host lists (`VICTORY_LIST_IDLE=256`:
the 1,400 lists alive are in use, not idle), root-view windows reusing a
covering window (no less memory, preparing slower).

## Landing on 3 threads; profile of the list threads (2026-11-26)

`play.ps1 -RX7900XT` also sets `VICTORY_LAND_THREADS=3` (4 before): 39.0 ms
preparing a frame at about 10,000 draws against 41.0 (release, free roam).
3 list-call threads with 2 compare threads made preparing cheaper but the
frame longer: the list calls became the wall (73,500 calls a frame, 76 ms of
each of the 3 threads). View pools at 4 threads: worse.

Sampled list threads (`VICTORY_TIMELINE_LISTS=1`, new, diagnostics): 7 %
keeping slots' resources, 6 % binding, 6 % in thread-local tables, 5.7 %
waiting for new card memory to be mapped (kept: without it the card faulted
in avatarmanz's runs), 4 % the D3D12 runtime, about 5 % hashing. Descriptions
are now hashed eight bytes a step with our own hasher; no change measured.

## Fewer worker threads for 16 hardware threads (2026-11-26)

`play.ps1 -RX7900XT` now sets `VICTORY_THIN_THREADS=4` (list calls
translated, 8 before) and `VICTORY_UNDER_THREADS=3` (title bytes under
flights' writes compared, 6 before). The defaults were measured on a 24-thread
i9-12900K; on the 7800X3D's 16 they took the processors from the title's own
threads. Release build, free roam at the user's save, preparing a frame at
like draws (interleaved runs, Parsec capturing in all):

| draws a frame | 4 and 3 threads | 8 and 6 |
|---|---|---|
| 5,600-5,900 | 41.1 ms | 43.3-47.8 |
| 7,200 | 42.1 | 48.6 |
| 8,000-8,500 | 43.3-44.3 | 49.9 |
| 10,000-10,400 | 40.1-41.0 | 51.9-55.7 |

Walking in the lighter views: 19.7 fps (slowest tenth 17.8). Tag
`goal-20fps-step5`.

## Play the release build (2026-11-26)

Free roam at the user's save, release against diagnostics build, run after
run (Parsec capturing the screen in both): walking 6.5 against 7.9 ms a
frame per 1,000 draws, standing 17.0 fps (slowest tenth 16.0) at 6,100 draws.
To play: `cargo build --release --locked -p victory`, `victory shaders` with
the release executable, then `scripts\play.ps1 -RX7900XT -Build release`.

## codec_d's cool_fox merged (2026-11-26)

The six commits of codec_d's fork (published as `JordanFromIT/cool_fox`, on
the same avatarmanz base) taken as they are, but for their launcher (it runs
their private prebuilt executable) and README: memory budgets for PCs with
32 GB of RAM (texture staging bounded under memory pressure, pools that back
off when Windows refuses them, uploads deferred under low commit), descriptor
slots released when the card finishes, the bindless pass per heap,
`VICTORY_INDIRECT_REST` off by default, NVIDIA Wave32 lowering, fixes for the smaller console model.
Two tests of the indirect budget assumed `VICTORY_INDIRECT_REST` on: they ask
for it now (or skip without it). After the merge, `victory shaders` again (the
translator changed). Tag `goal-20fps-step4`.

Free roam, the user's save outside the house (median / p10 / slowest):

| | standing | walking | frame, average |
|---|---|---|---|
| step 3 | 17.6 / 10.9 / 1.8 | 15.0 / 9.8 / 7.8 | 105 / 76 ms |
| + cool_fox | 17.3 / 16.8 / 14.6 | 17.2 / 16.7 / 16.4 | 59 / 59 ms |

The free-roam view the walk faces sets the draws (6,000 to 13,000 a frame):
compare like with like.

## Tried after step 3, not kept (2026-11-26)

- Windows' segment heap for the process (an application manifest): no
  better, noisier. mimalloc for our own allocations was not tried (a new
  dependency).
- cool_fox's launcher settings `VICTORY_CARD_BUFFERS_MIN_KB=64` with
  `VICTORY_LEVELS_ON_DEMAND=1`, release build, free roam: walking 98 ms of
  preparing a frame and stalls to 1.8 fps (against 52 ms without).
- Early rounds of the heaps' records in parts (a quarter of each heap a
  round): the submission waited longer (9.9 ms against 6.3).
- Kept for looking: `VICTORY_TIMELINE_SUBMIT=1` samples the submitting thread
  in a timeline. Its profile: about a quarter waiting (heaps' records, pools,
  locks), 6 % asking the write watch, 8 % copying bytes, about 9 % in the
  system heap's allocation and release.

## The measured settings in the launcher (2026-11-26)

`play.ps1 -RX7900XT` now also sets `VICTORY_ASK_EARLY=2`,
`VICTORY_HEAP_SYNC_EARLY=1`, `VICTORY_JOIN_PARTS=16`, `VICTORY_JOIN_LISTS=96`
and `VICTORY_TITLE_PRIORITIES=1` (the title's idle threads at below normal;
`play.ps1` alone keeps `0`). Tag `goal-20fps-step3`.

| | idle | driving (65 s) | camera spin |
|---|---|---|---|
| `goal-20fps-start` | 21.2 / 20.5 | 17.8 / 11.7 | - |
| `goal-20fps-step3` | 23.0 / 22.5 | 19.4 / 16.5 | 17.7 / 15.9 |

No better: 2 MiB windows (`VICTORY_WINDOW_KB=2048`), waiting awake in the
wide pool (`VICTORY_WIDE_SPIN=200`) or in the landing pools (tried in code,
not kept: the processors are busy enough that spinning took them from the
title), GPU timing (`VICTORY_GPU_TIMING=1`: 5 fps, for looking only).

## Settings measured after step 2 (2026-11-26)

Over the first 65 s of driving (median / p10) and 2 min idle:

| | idle | driving |
|---|---|---|
| step 2, `VICTORY_ASK_EARLY=2` | 22.5 / 22.0 | 19.4 / 17.3 |
| + `VICTORY_HEAP_SYNC_EARLY=1` | 22.8 / 22.6 | 19.4 / 16.4 |
| + `VICTORY_JOIN_PARTS=16`, `VICTORY_JOIN_LISTS=96` | 22.8 / 22.2 | 20.0 / 15.3 |

No better: `VICTORY_MAP_THREADS=4`, `VICTORY_VIEW_THREADS=8`, the slots'
locks by a hash of the index (the heaps' records waited longer: 9.3 ms
against 6.3). The driving path differs from run to run (4,000 to 5,500 draws),
so differences under about 1 fps there are noise.

## Root views' reach asked once a second (2026-11-26)

An ask without a length (a root view) is given the title memory committed
around it (the intro's device removal), found by asking Windows region by
region. It was asked at every such lookup, from the threads that translate
list calls, while the title maps and unmaps pages: 680 queries in 8 frames
of driving, 362 ms of those threads. The answer for a range is now used
again for 1 s (`VICTORY_ROOT_REACH_MS`, 0: every time). Tag
`goal-20fps-step2`.

| | idle | driving |
|---|---|---|
| step 1 | 20.3 / 19.7 | 18.9 / 17.8 |
| + this change | 22.5 / 22.0 | 19.6 / 16.0 |

Tried, no better: the smaller console model's profile (`--console series-s`): as many draws,
slower.

## Window lookups share the lock (2026-11-26)

With windows finer than their 4 MiB cell (`VICTORY_WINDOW_KB=1024`, in
`play.ps1 -RX7900XT`), every lookup that may take any window holding a range
searched under the windows' write lock, so all threads asking stood in one
line: 1.4 s of waits in 4 frames of driving (timeline, all threads). The
search is now made under the read lock first; the write lock only makes a
window. The memory-line statistics read under the read lock too.

Self-test (boat mission, 120 s idle then 90 s driving; median / p10 fps):

| | idle | driving |
|---|---|---|
| before (`goal-20fps-start`) | 21.2 / 20.5 | 17.8 / 11.7 |
| + `VICTORY_ASK_EARLY=2` | 21.7 / 19.9 | 18.1 / 14.6 |
| + this change | 20.3 / 19.7 | 18.9 / 17.8 |

Tried, no better: `VICTORY_THIN_THREADS=12`; `VICTORY_LAND_THREADS=6` with
`VICTORY_VIEW_THREADS=6`.

## goal-20fps-start (2026-11-26)

The point the 20 fps work starts from: `scripts\play.ps1 -RX7900XT`, about
18-19 fps median in the world (run 7), peaks in the mid 20s. Tag
`goal-20fps-start`.

Before it, in order (see `git log`):

- A root view's window reaches the title memory committed around it (the
  device removal in the intro).
- Host lists' command allocators can be replaced every N resets
  (`VICTORY_ALLOCATOR_RESETS`, off: it gave back 2 MB).
- F11 saves the next 8 frames in diagnostics builds (`VICTORY_CAPTURE`).
- The RX 7900 XT settings (`play.ps1 -RX7900XT`), the notes in
  `docs/rx7900xt.md`, the README.
- Tried and not kept: ForwardVI's card memory (local branch `forwardvi-port`).
