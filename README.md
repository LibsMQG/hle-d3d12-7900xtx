# hle-d3d12-7900xtx

A compatibility runtime that runs x64 PE executables built for another platform natively on
Windows: high-level emulation of their system services, a Direct3D 12 GPU back end and an
RDNA-to-DXIL shader recompiler. A fork of
[YukiRyuu/RYUKI-7900XT](https://github.com/YukiRyuu/RYUKI-7900XT), itself built on
[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) and
[VIctory](https://github.com/Marvisak/VIctory). Developed and measured on an
**RX 7900 XTX with 32 GB of RAM**.

Maintained by **libs**: the ports, measurements, diagnostics and launcher presets of this fork.

## Results at a glance

This fork (`main` at `1ef9e07`, `play.ps1 -Tuned`) against the RYUKI base it started from
(its own tuned settings), same PC, same fixed test spot standing still, diagnostics builds,
no errors in the log, picture complete in both.

| | RYUKI base | This fork | Gain |
|---|---|---|---|
| Frame time (median) | 78.8 ms (p90 87.7) | **64.7 ms** (p90 68.1) | **-18 %** |
| Frame rate | 12.7 fps | **15.5 fps** | **+22 %** |
| CPU: submitting thread's work per frame | 75.5 ms | **48.1 ms** | **-36 %** |
| CPU: title waiting at presents | 28.1 ms | 19.5 ms | -31 % |
| GPU board power (median) | 273 W | **199 W** | **-74 W** |
| GPU activity / graphics clock | 100 % / 3.08 GHz | 70 % / 1.99 GHz | same work, less effort |
| Write-combined memory committed | 13.7 GB | 12.9 GB | -0.8 GB |
| Process RAM: working set / private | 10.8 / 27.5 GB | 11.2 / 26.7 GB | about the same |
| VRAM in use | 11.0 GB | 11.2 GB | about the same |

The frame is limited by the CPU thread that turns the title's command lists into Direct3D 12
work, not by the GPU: most of the gain is that thread doing less. Power, activity and clock come
from the card's sensors (4 samples a second); the base also keeps the card's clocks up on
purpose, which is part of the power difference. One run per side (40-60 s): the same build
varies by about 15 ms from run to run.

**This repository contains no game files, code or assets.** You need your own copy of the
software you run with it. The asset policy ([docs/asset-policy.md](docs/asset-policy.md)) is
checked by `cargo xtask policy` and by the pre-commit hook (`cargo xtask install-hooks`).

Contents: [Results](#results-at-a-glance) · [PC](#the-pc-it-is-measured-on) ·
[Milestones](#milestones) ·
[Where the code comes from](#where-the-code-comes-from) ·
[Our own work](#our-own-work) ·
[Known limits](#known-limits) ·
[How to run it](#how-to-run-it) ·
[Credits](#credits)

## The PC it is measured on

| | |
|---|---|
| GPU | AMD Radeon RX 7900 XTX, 24 GB |
| CPU | AMD Ryzen 9 5900X |
| RAM | 32 GB DDR4-3600 |
| OS | Windows 11 Pro |

## Milestones

Standing still in one fixed test spot, release or diagnostics build, median frame time.

| Date | Where it stood |
|---|---|
| 2026-11-25 | First run on this PC: menus at 30 fps; entering the world took about two minutes of pipeline compilation; in the world 0.1-1.8 fps, with about 16% of the draws dropped for lack of a pipeline. |
| 2026-11-26 | The project's own first line, with its speed switches: 143 ms a frame (7 fps). |
| 2026-11-26 | Rebased on the RYUKI tree, with this fork's fixes and diagnostics on top: 73 ms (13.6 fps), picture complete once the pipeline cache is built, no errors in the log. |
| 2026-11-27 | Renderer ports 1 and 5, then the RDNA3 fork's submit-thread changes and buffers in place: about 70 ms; the submitting thread's work went from ~77 to ~57 ms a frame. |
| 2026-11-27 | Renderer port 2 (descriptor stores tell their slots): **about 68 ms (14.7 fps)**, submitting thread ~51 ms; no picture defects standing, walking or moving fast. |
| 2026-11-27 | Descriptor records compared in place, dynamic bits, prelude pool: **64.7 ms (15.5 fps)**, submitting thread 48 ms, GPU at 199 W instead of the base's 273 W ([Results at a glance](#results-at-a-glance)). |

Moving fast through the world is still far slower (about 6-12 fps): pipelines met for the first time
and memory pressure, see [Known limits](#known-limits).

## Where the code comes from

Every commit taken from elsewhere keeps its author and, where it was a cherry-pick, a
`cherry picked from` line. Each fork was read and checked before anything was taken.

| Source | What it is | What we took, and why |
|---|---|---|
| [YukiRyuu/RYUKI-7900XT](https://github.com/YukiRyuu/RYUKI-7900XT) | 7900 XT/XTX fork with 32 GB work and many community forks merged | **The base tree** (the root commit). Chosen because it was already faster than our first line, so our work was ported onto it piece by piece. |
| [avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) | 7900 XTX fork that carries the upstream renderer patches | **Renderer ports 1, 5, 6, 7** and the halving search for ranges asked ahead. Aim: the submitting thread, which is what limits the frame. Ports 1 and 5 are on in `play.ps1 -Tuned`; 6 and 7 stay off (measured: no gain or worse). |
| [polackangry/POLSKAGUROM](https://github.com/polackangry/POLSKAGUROM) | NVIDIA-oriented fork on the upstream line | **Renderer port 2** (`VICTORY_PORT_DESCRIPTORS`): the title's descriptor-record stores report their slots instead of the whole view heap being scanned. Submitting thread -4 ms, heap rounds 28 -> 19 ms. On in `-Tuned`. |
| [XaviFortes/VIctory-RDNA](https://github.com/XaviFortes/VIctory-RDNA) | RX 7900 XT fork focused on the submitting thread | AVX2 compare in `Shadow::take`, a lock-free "anything pending?" flag with a `RwLock`, 4 MiB journal pieces, `VICTORY_PORT_BUFFERS` (title buffers above 4 GiB in place, big ones kept in VRAM). Together about -5 ms a frame. Later `VICTORY_UNGUARD_RUNS` (guarded blocks given back with one protection change a run; on): in two drives its phases 2-4% below their neighbours, inside the noise. Its unconditional in-place card-step changes and the per-part buffer spans were left out (no effect here or twice the cost; correctness risk). |
| Op-lite's fork | Community fork with memory hooks and work on freezes | **`VICTORY_SOFT_VIEWS=copies`** (ported by hand): the views the title's batch page-mapping copies make at its GPU view are kept in our books and mapped only when something touches them, in place of a map and an unmap call a page. In two drives the remapping fell from 316 to 118 ms a second and the slow frames by half (p99 457 -> 271 ms). On in `-Tuned`. Its hooks that patch the title's memory calls were left out (stale textures by its own account, or retired). |
| [zeex64/victory-7900xtx](https://github.com/zeex64/victory-7900xtx) | Fork on the upstream line, run under Linux/Proton | **`VICTORY_SYNC_IN_PLACE`** (ported, adapted to port 2's parts): the heaps' descriptor records read where they are and compared with AVX2 as read, in place of a copy of each changed page first. Reading and comparing 4.6 -> 3.3 ms a frame, the submitting thread's wait for it -0.4 ms. On in `-Tuned`. Its other speed switches were already here (`VICTORY_SLOT_BITS`) or turned off by its own measurements; its Linux and Wine work does not apply. |
| [WillyboPeep/victory-all-forks](https://github.com/WillyboPeep/victory-all-forks) | A merge of many community forks | **Correctness fixes**, not speed: use-after-free in audio, queues and fences; save-data validation; loader, GPU, shader-recompiler and DirectStorage fixes; frame-wait ticks; Bink skip. Full list under [Fixes taken from others](#fixes-taken-from-others). |
| [VIctory](https://github.com/Marvisak/VIctory) | The upstream project | Through all of the above. One idea re-written by us: a submission's one asking holds every range over both its parts (after upstream `e000e02`). |

Forks that were read and checked but gave **nothing** taken here (already present, no gain on
this GPU, or a different architecture): Belobug/VIctory-Belobug-AMD, fxceless/victory-fork,
JoshuuDev/victory-amd, DexNom, rorodev6's forks,
Paradoxo321/rodrion-vi, LooperSalty's fork and others.

### Fixes taken from others

- **Use after free**: the audio heap freed under the audio decoder; command queues released while
  their held work was being run; fences released while queue waits, signals or late signals still
  held them.
- **Saves**: the blob count is validated before it is read; a second change of the same blob in one
  update is refused; replaced blobs count before the quota is clamped; a failed create leaves a
  null handle; a legacy container is not migrated when one of its blobs cannot be read.
- **Loader**: a module is found whatever the case of its file name; a `clzero` loop is decoded only
  from its real `cmp rax, reg` form.
- **GPU**: a released view heap's slot run is forgotten (a heap made at the same address got stale
  slots); a null constant-buffer write replaces the old descriptor; a query heap with no queries
  is refused; an in-place placement gives back exactly the pages it locked; textures keep their
  4 KiB alignment; snapshot allocations are retried after idle buffers are let go; two frame waits
  that wake together take two ticks.
- **Shader recompiler**: an AMD shader-compiler crash (a handle annotation reused across blocks);
  `v_div_fixup_f32` for 0/x and x/Inf; `v_readfirstlane` in helper lanes; float compares that are
  always true or false; 16-bit gradients on gfx10; PSV0 semantic names repeated; first-lane and
  lane-read refusals retried wave-wide; export target 16 read as `pos4`.
- **DirectStorage**: the factory's out pointer is cleared on failure.
- `VICTORY_SKIP_BINK=1` skips Bink intro movies (`all` also the looping ones).

## Our own work

What this fork (libs) wrote itself, not taken from another tree.

### Diagnostics

All off by default and counting only; release builds have them too.

| Switch | Says |
|---|---|
| `VICTORY_CPU_STATS=1` | every 5 s, per frame: the submitting thread's waits for work in flight, by what it was about to read and why the card's steps did not take it; how many of the bytes waited for really changed; fills of upload memory by who made them; the write watch's `VirtualQuery` calls |
| `VICTORY_DISK_STATS=1` | the DirectStorage read report (requests, MB/s, workers) at info level |
| `VICTORY_MEM_OWNERS=<csv>` | upload and shared buffers with the code that asked for them; every 30 s the write-combined and private memory blocks by size and owner |
| `VICTORY_CHANGED_BYTES=1` | (diagnostics build) how much of what is copied into host copies differs from what they held |
| `VICTORY_PORT_DESCRIPTORS_SORT=1` | sorts the view records renderer port 2's stores did not report: by kind, by whether a near slot was reported, by whether the bytes copy a reported record |
| `VICTORY_PORT_DESCRIPTORS_TRACE=1` | a hardware write-breakpoint tracer (DR0-DR3 on every thread) that names the code writing records port 2 missed |

`diagnostics/` holds the probes and scripts that read these counters and measure the host.

What they showed: the frame is limited by the submitting thread (preparation plus waits for
the card, one after the other), not by RAM or VRAM traffic; port 2's real misses are about
6 records a frame out of 65,000 and none of them is a CPU write through the heap's address.

### Speed and correctness changes

- **Descriptor slot bits** (`4b77a27`): per-slot bits keep two hot locks out of writes that
  change nothing there (-1.9% frame time, in-run A/B).
- **Every port re-done for this tree**: the renderer ports and the RDNA3 changes were written for
  other histories; each one was adapted here (in-place page budget, deferred unmappings, remap
  marks, the absence of upstream modules), and each commit says what was changed.
- **Switches for always-on changes** (`VICTORY_TAKE_AVX2`, `VICTORY_PENDING_FLAG`,
  `VICTORY_WIDE_SMALL`) so each one could be measured: 256 KiB wide jobs turned out slower and
  are now opt-in; port 2 can be flipped inside a run. Changes were measured with the base
  tree's in-run A/B (`VICTORY_AB`), since the same build varies ~15 ms from run to run.
- **Measured defaults in `play.ps1 -Tuned`**: card clocks no longer kept up
  (`VICTORY_CARD_AWAKE` off); ports 1, 5, 2 and `VICTORY_PORT_BUFFERS` on; ports 6 and 7 off.
- **One fix reverted**: the quad-read derivative change from the all-forks merge was taken out
  again on this tree.
- **CI**: clippy clean with `-D warnings` in both CI variants; tests that share global state
  (watch, call hooks) run one at a time, so the suite no longer fails at random.

## Known limits

- Ray-traced light is off by default: the stages of the title's ray-tracing structure builder (a
  few hundred dispatches a frame) are dropped, and an estimate (`VICTORY_AMBIENT`) stands in for
  the indirect light. `scripts\play.ps1 -RayTracing` runs the builder
  ([docs/fork-ray-tracing.md](docs/fork-ray-tracing.md)): lit interiors at a cost in card time,
  not yet measured on this tree. These stages are the only dispatches the runs drop, apart from
  pipelines still being compiled.
- Two device methods that create pipelines from native code pieces are not implemented; the title
  then makes the same pipelines through the stream method (measured), so no draw is lost. The 203
  shaders the recompiler cannot translate (geometry shaders of one voxelization pass, three text
  programs, five shadow tracers) are in no pipeline the title draws or dispatches with.
- Moving fast through new areas (driving): pipelines are compiled when first met (`victory warm
  --used` builds them ahead), and with 32 GB of RAM free memory falls to 1-2 GB. The title's
  memory remapping was 300-850 ms a second; with `VICTORY_SOFT_VIEWS=copies` (on in `-Tuned`)
  about 120, nearly all of it the title's own mapping calls, and driving runs at about 9 fps.
- After any change to the shader recompiler the first start rebuilds the pipeline cache: several
  minutes with missing textures before the picture is complete.

## How to run it

See [docs/README-ryuki.md](docs/README-ryuki.md) for the requirements and the page-file setting a
32 GB PC needs. In short:

```powershell
copy victory.example.toml victory.toml      # then set your paths, outside this folder
cargo build --locked --release -p victory
scripts\play.ps1 -Tuned -Build release -Exe <path of your copy's executable>
```

Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with shader model 6.6 and mesh shaders,
Rust (the toolchain is pinned) and the MSVC build tools.

## Credits

A tree of other people's work. The history is kept, and commits taken from elsewhere keep their
authors.

- **libs**: this fork: everything under [Our own work](#our-own-work), the ports from the forks
  below, the tools in [diagnostics/](diagnostics/README.md) and [scripts/play.ps1](scripts/play.ps1).
- **[YukiRyuu/RYUKI-7900XT](https://github.com/YukiRyuu/RYUKI-7900XT)** by ryuki: the base of this
  fork, its 32 GB work and the forks it carries (its full list is in
  [docs/README-ryuki.md](docs/README-ryuki.md): avatarmanz, fxceless, DexNom, Belobug, FQY55kai,
  myokex, codec_d's cool_fox, Op-lite and others).
- **[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)** by avatarmanz,
  for the renderer ports.
- **[polackangry/POLSKAGUROM](https://github.com/polackangry/POLSKAGUROM)** by polackangry and
  Muhammad Salman Barkat, for renderer port 2.
- **[XaviFortes/VIctory-RDNA](https://github.com/XaviFortes/VIctory-RDNA)** by XaviFortes, for the
  submit-thread changes, `VICTORY_PORT_BUFFERS` and `VICTORY_UNGUARD_RUNS`.
- **Op-lite's fork** by Op-lite, for `VICTORY_SOFT_VIEWS`.
- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its contributors.
- **[WillyboPeep/victory-all-forks](https://github.com/WillyboPeep/victory-all-forks)**, a merge of
  community forks, where the fixes taken here were found, by AscilCH, C-GDev, David, DocBlack89,
  Duff, EatCodeSleep2, HailLucifer666, Jordan N, Peppacodes, Vax, avt0m4t, davutsimsekk,
  mynzjerko, quaternion, towiice and uiliannn.
- This fork's own changes were made with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file that is not covered is named in
[docs/asset-policy.md](docs/asset-policy.md).
