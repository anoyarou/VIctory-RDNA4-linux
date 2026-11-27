# Linux scripts (Wine/Proton, RX 9070 XT)

The scripts this branch is run with on Linux, as measured in [docs/linux-wine.md](../../docs/linux-wine.md).
They differ from `scripts/play.sh` (avatarmanz/VIctory-7900XTX#440): they use Proton's Wine with an existing
Proton prefix and the native `d3d12`/`dxgi` set by `WINEDLLOVERRIDES`, run inside gamescope, and carry the
settings measured fastest here (`settings.sh`), not `play-1080p.sh`'s renderer patches, which are slower on
this card.

| Script | What it does |
|---|---|
| `build-xwin.sh` | Cross-builds `victory.exe` with cargo-xwin; `--diag` for logs and the panel, `--deploy-dir <dir>` to copy it where `run.sh` starts it |
| `run.sh` | Starts the title under gamescope (windowed, borderless or fullscreen) with `settings.sh`; `VICTORY_FSR_4K=performance` for PC FSR 4.1.2 (see its header for the DLLs it needs) |
| `run-logged.sh <name>` | `run.sh` with a timestamped console log and a memory log, after stopping what is left of an earlier run in the prefix |
| `settings.sh` | The `VICTORY_...` settings, each with why; anything set in the environment wins |
| `memwatch.sh` | Memory every 5 s; stops the game if available memory and free swap fall under 3 GiB |

## Once

1. Build: `scripts/linux/build-xwin.sh --diag --deploy-dir ~/Games/victory`
2. Beside `victory.exe` in that folder: Microsoft's 64-bit `d3dcompiler_47.dll`, and a `d3d12core.dll` of
   vkd3d-proton built with dxil-spirv `694cc674` or later if your Proton's is older (docs/linux-wine.md, "Setup").
3. A Proton prefix (any prefix Proton has created, e.g. by running a Windows program once with it).
4. Preferably a Proton whose wineserver has the view-tree patch (Wine bug 60868; CachyOS/wine-cachyos#50).

## Each time

```bash
TITLE_EXE=/path/to/your/copy/<title executable> PROTON_DIR=/path/to/proton WINEPREFIX=/path/to/prefix scripts/linux/run-logged.sh first-run
```

`TITLE_EXE` is the full path to the title executable in your copy; it has no default. Defaults when unset: `VICTORY_DIR=~/Games/victory`, `WINEPREFIX=~/Games/victory-prefix`, `PROTON_DIR` = Steam's
"Proton - Experimental". Window: `GAMESCOPE_MODE=windowed` (default), `borderless` or `fullscreen`, with
`GAMESCOPE_OUT_W/H` for the screen size and `GAMESCOPE_FILTER=fsr` to scale up; Super+F toggles fullscreen.
Needs `gamescope`, `gawk` (for the log's timestamps) and 32 GB of RAM with swap.

Nothing of the title goes into this repository: keep `LOG_DIR` (default `VICTORY_DIR/logs`) and your copy outside it.
