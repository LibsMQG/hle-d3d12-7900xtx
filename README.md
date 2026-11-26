# VIctory on an RX 7900 XTX (RYUKI base, with community fixes)

A fork of [YukiRyuu/RYUKI-7900XT](https://github.com/YukiRyuu/RYUKI-7900XT) at `2f546ea`, itself built on
[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) and
[VIctory](https://github.com/Marvisak/VIctory): a compatibility layer that runs a console title
natively on Windows from files the user supplies. On top of RYUKI's tree this fork carries crash,
save and loader fixes from the community forks, a set of shader-translator fixes, and diagnostics
that say where the submitting thread's time goes. Measured on an **RX 7900 XTX with 32 GB of RAM**.

**This repository contains no game files, code or assets.** You need your own copy of the game.
It is not affiliated with or endorsed by the game's developer or publisher. The asset policy
([docs/asset-policy.md](docs/asset-policy.md)) is checked by `cargo xtask policy` and by the
pre-commit hook (`cargo xtask install-hooks`).

## The PC it was tested on

| | |
|---|---|
| GPU | AMD Radeon RX 7900 XTX, 24 GB |
| CPU | AMD Ryzen 9 5900X |
| RAM | 32 GB DDR4-3600 |
| OS | Windows 11 Pro |

## Where it stands (2026-11-26)

Measured on this PC, release build, standing in the same spot of the world, median of the last
16 five-second blocks once every pipeline was made:

| | Frame | Note |
|---|---|---|
| RYUKI `2f546ea` as published | 71.4 ms (14.0 fps) | `scripts\play.ps1 -RX7900XT -Build release` |
| This fork | 73.2 ms (13.6 fps) | one run, with the counters below switched on; 8% more draws in view |

0 errors in the log; draws without a host pipeline about 350 a frame on both (pipelines the title
makes with two device methods that are not implemented yet, and shaders the translator refuses).
After any change to the shader translator the first start rebuilds the pipeline cache: several
minutes with missing textures before the picture is complete.

## What this fork adds to RYUKI's tree

Diagnostics, all off by default, counting only (release builds too):

| Switch | Says |
|---|---|
| `VICTORY_CPU_STATS=1` | every 5 s, per frame: the submitting thread's waits for submissions in flight, by what it was about to read and why the steps on the card did not take it; per wait, how many of the bytes read the flights really changed meanwhile; fills of upload memory by who made them; the write watch's `VirtualQuery` calls |
| `VICTORY_DISK_STATS=1` | the DirectStorage read report (requests, MB/s, workers) at info level |
| `VICTORY_MEM_OWNERS=<csv>` | upload and shared buffers with the code that asked for them; every 30 s the write-combined and private memory blocks by size and owner |
| `VICTORY_CHANGED_BYTES=1` | (diagnostics build) how much of what is copied into host copies differs from what they held |

Fixes taken from the community forks (authors kept in the history): use-after-free of the audio
heap, of command queues released while their work was held, and of fences held in queue waits and
late signals; XGameSave blob count, duplicate changes, quota and failed creates; title DLLs found
whatever their name's case; a `clzero` loop decoded only from its real form; DirectStorage factory
out pointer; heap and null-descriptor handling; in-place pages; frame waits that wake together;
and shader-translator fixes (an AMD compiler crash, `v_div_fixup_f32`, helper lanes,
16-bit gradients, PSV0 names and more). `VICTORY_SKIP_BINK=1` skips the title's intro movies.

Which commits of the older line were taken, already present, deferred or skipped, and why, is kept
outside this repository with the project's notes.

## How to run it

As in RYUKI's tree ([docs/README-ryuki.md](docs/README-ryuki.md) has the details, including the
page-file setting a 32 GB PC needs):

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
  fork, its 32 GB work and the forks it carries (see [docs/README-ryuki.md](docs/README-ryuki.md)
  for its full list: avatarmanz, fxceless, DexNom, Belobug, FQY55kai, myokex, codec_d's cool_fox,
  Op-lite and others).
- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its contributors.
- **[WillyboPeep/victory-all-forks](https://github.com/WillyboPeep/victory-all-forks)**, a merge of
  community forks, where the fixes taken here were found, by AscilCH, C-GDev, David, DocBlack89,
  Duff, EatCodeSleep2, HailLucifer666, Jordan N, Peppacodes, Vax, avt0m4t, davutsimsekk,
  mynzjerko, quaternion, towiice and uiliannn.
- This fork's own changes were made with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file that is not covered is named in
[docs/asset-policy.md](docs/asset-policy.md).
