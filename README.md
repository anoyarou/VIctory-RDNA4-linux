# VIctory on RDNA 4 under Linux

A fork of [VIctory](https://github.com/Marvisak/VIctory), a compatibility layer that runs an Xbox
Series title on a PC from files the user supplies. This one runs it **on Linux under Wine/Proton, on
an AMD RDNA 4 card**. It is playable, slowly.

**Upstream is [Marvisak/VIctory](https://github.com/Marvisak/VIctory)**: fixes are taken from there,
and changes meant for everyone go there as pull requests. The tree started from
avatarmanz/VIctory-7900XTX with Belobug/VIctory-Belobug-AMD's RX 9070 XT fixes; both repositories no
longer open on GitHub (since 2026-11-27), but their work is in this history with its authors, and
references such as `avatarmanz/VIctory-7900XTX#…` still name what was taken.

**This repository contains no game files, code or assets.** You need your own copy of the game. It is
not affiliated with or endorsed by the game's developer or publisher.

## Where it stands (2026-11-28)

Tested on one machine:

| Part | Setup |
|---|---|
| CPU | AMD Ryzen 9 5950X (16 cores, 32 threads) |
| GPU | AMD Radeon RX 9070 XT, 16 GB (GFX1201) |
| RAM | 32 GB, plus swap |
| OS | Fedora 44, kernel 7.3.6, KDE Plasma on Wayland |
| Driver | Mesa 26.2.8 RADV |
| Wine | Proton Experimental `experimental-11.0-20261112` with the view-tree wineserver and [`patches/wine`](patches/README.md), in gamescope |
| Direct3D 12 | vkd3d-proton's `d3d12core.dll` with dxil-spirv `b540c8a2` and [`patches/vkd3d-proton`](patches/README.md) |
| Upscaler | PC FSR 4.1.2: 1920x1080 inside, 3840x2160 out; FSR frame generation on |

| Scene | Frame time (rendered) | Shown with frame generation |
|---|---|---|
| Standing outdoors (~6,500-8,700 draws) | 56-60 ms (17-18 fps) | ~34-36 fps |
| Busy scenes (~7,500-9,500 draws) | 80-86 ms (12 fps) | ~24 fps |
| Memory | game process 14-16 GB resident | |

Every rendered frame gets one generated frame. It looks smooth, but input still follows the rendered rate.

Faces, skin and clothes look right, plants no longer pop, night skies are right, the picture is sharp
with FSR 4, and the game's UI stays steady with frame generation. The CPU sets the frame, not the card:
one thread prepares and submits the title's work.

Known to be wrong: with frame generation the edges of the screen flicker and full-screen effects such as the vignette can tear; ~330 draws a
frame are dropped (the ray-tracing builder is off), so indoor light is an approximation; loading the
world takes over 40 s, and memory grows with play time.

## How to run it on Linux

Built on Linux with [cargo-xwin](https://github.com/rust-cross/cargo-xwin); no Windows machine needed.

```bash
git clone https://github.com/anoyarou/VIctory-RDNA4-linux
cd VIctory-RDNA4-linux
cp victory.example.toml victory.toml      # then set your paths, outside this folder
scripts/linux/build-xwin.sh --diag
```

What you need (Proton, the patches, the DLLs), the launch command and the settings that matter:
**[docs/linux-wine.md](docs/linux-wine.md)**. Scripts: [scripts/linux/](scripts/linux/README.md).
Optional PlayStation button prompts: [docs/playstation-prompts.md](docs/playstation-prompts.md).
Finding a picture fault by capturing and replaying a frame: [docs/replay.md](docs/replay.md).

## Credits

This is a tree of other people's work with a little on top. The history is kept, and commits taken
from elsewhere name the commit and its author.

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its contributors: the loader, the
  runtime services, the Direct3D 12 back end and the shader recompiler.
- **avatarmanz/VIctory-7900XTX**: the base of this branch (how it differs, measured on Windows:
  [docs/findings.md](docs/findings.md)), and the Linux testers' pull requests and reports
  (#13, #59, #111, #148, #166, #200 and others); the face fix of #114 with Beqren's raw typed loads.
- **beloooo (Belobug/VIctory-Belobug-AMD)**: the RX 9070 XT fixes and HUD (`81ebb7b`, `e942896`),
  prelude texture uploads (`5d6bd84`), the FSR 4.1.2 bridge (`cd8dfc0`), the ambient light idea and
  pipeline workers, and work by romainhedouin.
- **tippythetaidum** (fxceless/victory-fork#927): the half views that make faces right on Radeon;
  **bordersky/victory-linux** and **Fedalv** for testing the face pieces together under Linux.
- **borntofrag34**: the Wine branch of avatarmanz/VIctory-7900XTX#440; **LloydNicholson**: the plant fix
  (#437); **icecreammule277-dot**: `VICTORY_WC_NO_BACK` (Belobug/VIctory-Belobug-AMD#477); the authors of
  avatarmanz/VIctory-7900XTX#423, #429, #443, #446, #447, #448 and Belobug/VIctory-Belobug-AMD#512, #518.
- **[Slaktarn700/SlaktarnVI](https://github.com/Slaktarn700/SlaktarnVI)**: FSR frame generation and its
  UI-layer split (`e5b0dc81`); **[MTrv0/VIctory-AMD-UM](https://github.com/MTrv0/VIctory-AMD-UM)**: stale
  texture slots brought up to date (`0074263a`); **[crossducking/RAZDA4A-XEN](https://github.com/crossducking/RAZDA4A-XEN)**:
  the lighting settings.
- **[maren-halv/VIctory-Fast](https://github.com/maren-halv/VIctory-Fast)**: watch-journal and heap-pass
  changes (`79a92c7`, `7da8eb5`).
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**, **[DexNom/VIctory](https://github.com/DexNom/VIctory)**,
  **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**,
  **[myokex/victory-amd](https://github.com/myokex/victory-amd)** (with Spexivus, joroyt, tightfall and
  Joshua Odom), **Geckomacabre** and **cxy0072019-sketch**: fixes carried through the base.
- **[HansKristian-Work/dxil-spirv](https://github.com/HansKristian-Work/dxil-spirv)** and
  **vkd3d-proton**: the shadow-pipeline fix (`694cc674`) and FSR 4 under Vulkan.
- **Kenney** and **Xelu** (both CC0): the PlayStation prompt art `victory glyphs` builds; not stored here.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was read for how the title behaves under another
  layer; none of its code is here.
- This fork's changes were made with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file not covered is named in
[docs/asset-policy.md](docs/asset-policy.md).
