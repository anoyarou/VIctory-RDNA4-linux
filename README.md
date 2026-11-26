# VIctory on RDNA 4 under Linux

A fork of [VIctory](https://github.com/Marvisak/VIctory), a compatibility layer
that runs an Xbox Series title on a PC from files the user supplies. This one is
about running it **on Linux under Wine/Proton, on an AMD RDNA 4 card**. Its base is
[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) (speed on
RDNA 3, Windows) with [Belobug](https://github.com/Belobug/VIctory-Belobug-AMD)'s
RX 9070 XT fixes (`rx9070xt` branch), plus what the Wine runs here needed, taken
from those repositories, from Marvisak/VIctory and from our own measurements. It
is playable, slowly.

**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by the game's developer
or publisher.

## Where it stands (2026-11-26)

Tested on one machine only:

| | |
|---|---|
| CPU | AMD Ryzen 9 5950X (16 cores, 32 threads) |
| GPU | AMD Radeon RX 9070 XT, 16 GB (Navi 48, GFX1201) |
| RAM | 32 GB, plus 32 GB of swap (8 GB zram, 24 GB file) |
| OS | Fedora 44, kernel 7.3.4, KDE Plasma 6.8.3 on Wayland |
| Driver | Mesa 26.2.7 RADV |
| Wine | Proton Experimental `experimental-11.0-20261112` with a patched wineserver (view tree, Wine bug 60868), inside gamescope at 1600x900 |
| Direct3D 12 | vkd3d-proton's `d3d12core.dll` rebuilt with dxil-spirv `b540c8a2` (see below) |
| Profile | Series X, from a save |

| | |
|---|---|
| Standing in the first playable area (~3,600 draws) | 61-64 ms a frame (16 fps) |
| Busier views (~9,000 draws) | 78-92 ms (11-13 fps) |
| Memory | game process 13.6-15.6 GB resident; 32 GB with swap was enough |

It started at ~575 ms a frame on this base (2 fps). What brought it down, run by
run, is in [docs/linux-wine.md](docs/linux-wine.md).

What is known to be wrong here:

- About 340 draws a frame are dropped without a host pipeline.
- The logo videos at the start jump back and forth.
- Lighting indoors is only approximate without ray tracing (an ambient stand-in).
- Loading the world takes more than 40 s.

## How to run it on Linux

Built on Linux with [cargo-xwin](https://github.com/rust-cross/cargo-xwin) for
`x86_64-pc-windows-msvc`; no Windows machine needed.

```bash
git clone https://github.com/anoyarou/VIctory-RDNA4-linux
cd VIctory-RDNA4-linux
cp victory.example.toml victory.toml      # then set your paths, outside this folder
cargo xwin build --profile diag --target x86_64-pc-windows-msvc -p victory --features victory/diagnostics
```

Then, with a Wine prefix of Proton's:

- `WINEDLLOVERRIDES="d3d12,d3d12core,dxgi,d3d11,d3d10core=n,b;d3dcompiler_47=n"`, and a
  64-bit Microsoft `d3dcompiler_47.dll` next to `victory.exe`.
- `VKD3D_CONFIG=ignore_shared_fence,descriptor_heap` (needs Resizable BAR).
- **A vkd3d-proton with dxil-spirv `694cc674` or later** (vkd3d-proton master
  `71cae09` has it). With older ones six shadow pipelines are never made and each
  frame waits 500 ms for them. Rebuilt `d3d12core.dll` next to `victory.exe` works
  without touching Proton or the prefix.
- A wineserver with the view-tree patch, or loading the world is far slower.
- The settings of `scripts/play.ps1` (not `-Safe`) as environment variables, plus
  `VICTORY_FETCH_CHECK=1` (without it the characters' skin looks wrong).
- `wine victory.exe run <your copy's executable> --execute --console series-x`.

All of it, with the reasons and the measurements, is in
[docs/linux-wine.md](docs/linux-wine.md). Commands and debugging switches:
[docs/README-fxceless.md](docs/README-fxceless.md); every `VICTORY_...` switch is
described in a comment where it is read.

## What the RX 7900 XTX base does differently

From avatarmanz/VIctory-7900XTX, in short; the reasons and the measurements (on
Windows) are in [docs/findings.md](docs/findings.md).

- **List calls are written down, not carried out, on the title's threads**
  (`VICTORY_THIN`): eight threads of ours translate them.
- **Submissions are prepared by a thread of ours and joined**
  (`VICTORY_ASYNC_SUBMIT`, `VICTORY_JOIN_PARTS`), with what can be spread over
  threads spread: refreshes, compares (AVX2), descriptor records, landings.
- **What shaders wrote comes back beside the write guards**, by the dwords that
  changed, on a thread of its own (`VICTORY_RELAY`, `VICTORY_LAND`).
- **Buffer fills that need bytes still on the card are done on the card**
  (`VICTORY_CARD_STEPS`) instead of waiting for it.
- **The title's writes are caught more cheaply**: one view opened at a fault,
  blocks that rest, places in the title's code that look before they write.
- **Textures go up on a copy queue of their own** and a slot shows one only after
  its upload ran, which also ended a device loss that came every 15 minutes or so.
- **Boats float**: textures that shaders write and the title's CPU reads go back
  to title memory (from fxceless and Geckomacabre, on by default here).
- **Frame record and replay** with per-pass times and shader variants compared
  turn by turn ([docs/replay.md](docs/replay.md)).

## Credits

This is a tree of other people's work with a little more on top. The history is kept, and
commits taken from elsewhere name the commit and its author.

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors: the loader, the runtime services, the Direct3D 12 back end and
  the shader recompiler.
- **[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)**:
  the base of this branch, and the pull requests and reports of the people testing
  it under Linux (avatarmanz/VIctory-7900XTX#13, avatarmanz/VIctory-7900XTX#59,
  avatarmanz/VIctory-7900XTX#111, avatarmanz/VIctory-7900XTX#148,
  avatarmanz/VIctory-7900XTX#166, avatarmanz/VIctory-7900XTX#200 and others).
- **beloooo ([Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD))**:
  the RX 9070 XT fixes of the `rx9070xt` branch (`81ebb7b`).
- **[HansKristian-Work/dxil-spirv](https://github.com/HansKristian-Work/dxil-spirv)**
  and **vkd3d-proton**: the dxil-spirv fix (`694cc674`) the shadow pipelines need.
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**: the fork the RX 7900 XTX base
  started from, and the people whose pull requests it carries.
- **[DexNom/VIctory](https://github.com/DexNom/VIctory)**: ray tracing fixes,
  cube gradients, sparse textures.
- **Belobug/VIctory-Belobug-AMD** also: the ambient light idea, pipeline workers,
  and work by romainhedouin.
- **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**.
- **[myokex/victory-amd](https://github.com/myokex/victory-amd)**: the guard over
  every view of a page (with Spexivus, joroyt and tightfall), and Joshua Odom's
  reads into title memory.
- **Geckomacabre** and **cxy0072019-sketch**: the texture read-back.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was read for how the same
  title behaves under another layer; no code of it is here.
- This fork's changes were made with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file that is not
covered is named in [docs/asset-policy.md](docs/asset-policy.md).
