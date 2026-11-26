# hle-d3d12-runtime

A compatibility runtime that runs x64 PE executables built for another platform natively on
Windows: high-level emulation of their system services, a Direct3D 12 GPU back end and an
RDNA-to-DXIL shader recompiler. A fork of
[YukiRyuu/RYUKI-7900XT](https://github.com/YukiRyuu/RYUKI-7900XT), which is built on
[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) and
[VIctory](https://github.com/Marvisak/VIctory). Developed and measured on an
**RX 7900 XTX with 32 GB of RAM**.

**This repository contains no game files, code or assets.** You need your own copy of the
software you run with it. The asset policy ([docs/asset-policy.md](docs/asset-policy.md)) is
checked by `cargo xtask policy` and by the pre-commit hook (`cargo xtask install-hooks`).

## The PC it is measured on

| | |
|---|---|
| GPU | AMD Radeon RX 7900 XTX, 24 GB |
| CPU | AMD Ryzen 9 5900X |
| RAM | 32 GB DDR4-3600 |
| OS | Windows 11 Pro |

## Milestones

| Date | Where it stood |
|---|---|
| 2026-11-25 | First run on this PC: menus at 30 fps; entering the world took about two minutes of pipeline compilation (some 600 a minute); in the world 0.1-1.8 fps, with about 16% of the draws dropped for lack of a pipeline. |
| 2026-11-26 | The project's own line, with its speed switches: 143 ms a frame (7 fps) standing in a fixed test spot. |
| 2026-11-26 | Rebased on the RYUKI tree, with this fork's fixes and diagnostics on top: **73 ms a frame (13.6 fps)** in the same spot, picture complete once the pipeline cache is built, no errors in the log. |

Measured in a release build, standing still, median of the last 16 five-second blocks.

## What this fork adds

### Diagnostics

All off by default and counting only; release builds have them too.

| Switch | Says |
|---|---|
| `VICTORY_CPU_STATS=1` | every 5 s, per frame: the submitting thread's waits for work in flight, by what it was about to read and why the steps done on the card did not take it; per wait, how many of the bytes read the work in flight really changed meanwhile; fills of upload memory by who made them; the write watch's `VirtualQuery` calls |
| `VICTORY_DISK_STATS=1` | the DirectStorage read report (requests, MB/s, workers) at info level |
| `VICTORY_MEM_OWNERS=<csv>` | upload and shared buffers with the code that asked for them; every 30 s the write-combined and private memory blocks by size and owner |
| `VICTORY_CHANGED_BYTES=1` | (diagnostics build) how much of what is copied into host copies differs from what they held |

What they showed first: standing in the test spot, the submitting thread waits only about
2.6 ms a frame for work in flight, and the bytes it waited for were not changed by it; the rest
of its frame is preparation (buffer copies about 30 ms).

### Important fixes

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

`VICTORY_SKIP_BINK=1` skips Bink intro movies (`all` also the looping ones).

## Known limits

- Two device methods that create pipelines are not implemented yet: the draws that need those
  pipelines are dropped (about 350 a frame in the test spot), together with shaders the recompiler
  refuses.
- After any change to the shader recompiler the first start rebuilds the pipeline cache: several
  minutes with missing textures before the picture is complete.

## How to run it

See [docs/README-ryuki.md](docs/README-ryuki.md) for the requirements and the page-file setting a
32 GB PC needs. In short:

```powershell
copy victory.example.toml victory.toml      # then set your paths, outside this folder
cargo build --locked --release -p victory
scripts\play.ps1 -RX7900XT -Build release -Exe <path of your copy's executable>
```

Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with shader model 6.6 and mesh shaders,
Rust (the toolchain is pinned) and the MSVC build tools.

## Credits

A tree of other people's work. The history is kept, and commits taken from elsewhere keep their
authors.

- **[YukiRyuu/RYUKI-7900XT](https://github.com/YukiRyuu/RYUKI-7900XT)** by ryuki: the base of this
  fork, its 32 GB work and the forks it carries (its full list is in
  [docs/README-ryuki.md](docs/README-ryuki.md): avatarmanz, fxceless, DexNom, Belobug, FQY55kai,
  myokex, codec_d's cool_fox, Op-lite and others).
- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its contributors.
- **[WillyboPeep/victory-all-forks](https://github.com/WillyboPeep/victory-all-forks)**, a merge of
  community forks, where the fixes taken here were found, by AscilCH, C-GDev, David, DocBlack89,
  Duff, EatCodeSleep2, HailLucifer666, Jordan N, Peppacodes, Vax, avt0m4t, davutsimsekk,
  mynzjerko, quaternion, towiice and uiliannn.
- This fork's own changes were made with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file that is not covered is named in
[docs/asset-policy.md](docs/asset-policy.md).
