# Running it on Linux (Wine/Proton, AMD RDNA 4)

How to get the title running on Linux with this branch, and which settings matter. Everything here
was measured on one machine (see the [README](../README.md)); other setups may differ.

## What you need

| Piece | Why |
|---|---|
| Your own copy of the game | Nothing from the game is in this repository. |
| Proton Experimental, run in gamescope | The launcher in `scripts/linux/` expects both. |
| A wineserver with the view-tree patch (Wine bug 60868, CachyOS/wine-cachyos#50) | Without it frames take about twice as long and loading is far slower. |
| `patches/wine` built into that Proton (`win32u.so`, `wineserver`, `ntdll.so`) | Needed for the fastest setting below (port 1 as a dma-buf). Optional. Configure Wine with Vulkan, or `win32u.so` crashes at start. |
| vkd3d-proton with dxil-spirv `694cc674` or later, plus `patches/vkd3d-proton` | Older dxil-spirv loops on six shadow shaders and every frame waits for them. The patch is needed for port 1 as a dma-buf. Put the rebuilt `d3d12core.dll` next to `victory.exe`. |
| A 64-bit Microsoft `d3dcompiler_47.dll` next to `victory.exe` | Shader compiling. |
| Resizable BAR on | `VKD3D_CONFIG=ignore_shared_fence,descriptor_heap` needs it. |
| 32 GB of RAM plus swap | The game process takes 14-16 GB. |
| Optional: FSR 4.1.2 | `amd_fidelityfx_upscaler_dx12.dll` (FidelityFX SDK) in a folder you name with `VICTORY_FIDELITYFX_DIR`, and `amdxcffx64.dll` from AMD's Windows driver package in the prefix's `system32`. |
| Optional: frame generation | `amd_fidelityfx_framegeneration_dx12.dll` (FidelityFX SDK 2.4, `Kits/FidelityFX/signedbin`) in the same folder. Needs a diagnostics build. |

AMD's and Microsoft's DLLs are not in this repository; get them yourself.
How to apply and build the two patches: [patches/README.md](../patches/README.md).

## Build

On Linux, with [cargo-xwin](https://github.com/rust-cross/cargo-xwin):

```bash
scripts/linux/build-xwin.sh --diag
```

or by hand: `cargo xwin build --profile diag --target x86_64-pc-windows-msvc -p victory --features victory/diagnostics`.
The diagnostics build is the one measured here (frame generation and the stats panel need it).

## Run

`scripts/linux/run.sh` starts the title in gamescope with the settings of `scripts/linux/settings.sh`;
`run-logged.sh <name>` does the same and keeps a log. Set `TITLE_EXE`, `WINEPREFIX` and `PROTON_DIR`
first (see the scripts' headers). The fastest setup so far:

```bash
export VICTORY_PORT_CONSTANTS=1 VICTORY_PORT_SECTION=1 VICTORY_PORT_CONSTANTS_ORDER_BATCH=1
export VICTORY_AMBIENT=0.75 VICTORY_AMBIENT_AIR=1.5 VICTORY_AMBIENT_REFLECT=1.0
scripts/linux/run-logged.sh play
```

The first line needs the patched Wine and vkd3d-proton; without them leave it out (a few ms slower).

## Settings that matter

| Setting | Effect |
|---|---|
| `WINEDLLOVERRIDES="d3d12,d3d12core,dxgi,d3d11,d3d10core=n,b;d3dcompiler_47=n"` | Native Direct3D 12 and Microsoft's compiler (set by `run.sh`). |
| `VKD3D_CONFIG=ignore_shared_fence,descriptor_heap` | Without `descriptor_heap` the card faulted at world load. |
| `settings.sh` (`VICTORY_THIN=1`, `VICTORY_RELAY=2`, `VICTORY_CARD_STEPS=2` and others) | Took a frame from ~575 ms to ~300 ms. Keep them. |
| `VICTORY_FETCH_CHECK=1` | Needed with `RELAY=2`, or skin looks oily and eyes wrong. |
| `VICTORY_WC_NO_BACK=1` | Halves the copy-back time. |
| `VICTORY_CARD_BUFFERS_MIN_KB=64`, `VICTORY_CARD_BUFFERS_MB=4096` | A little faster, textures right. |
| `VICTORY_PORT_CONSTANTS=1` + `VICTORY_PORT_SECTION=1` + `VICTORY_PORT_CONSTANTS_ORDER_BATCH=1` | The fastest setup (about 13 % faster). Port 1's heap goes to the card as a dma-buf instead of a buffer the kernel walks at every submission, and its draws' clamps run in one pass. `PORT_CONSTANTS=1` alone, or without the patches, is slower than off. |
| `VICTORY_FSR_4K=quality\|balanced\|performance` with `FSR4_UPGRADE=1` | PC FSR 4.1.2 (`run.sh`; default `performance`: 1080p inside, 4K out). Without `FSR4_UPGRADE=1` Proton offers only FSR 3.1.5. |
| `VICTORY_FIDELITYFX_FRAMEGEN=1` | FSR frame generation (from Slaktarn700/SlaktarnVI). Roughly doubles the shown frames; flickers while moving at today's base frame rate, so not recommended yet. The game's UI and the stats panel are kept out of the interpolation (`VICTORY_FG_UI_LAYER=0`: not). |
| `VICTORY_AMBIENT=0.75`, `VICTORY_AMBIENT_AIR=1.5`, `VICTORY_AMBIENT_REFLECT=1.0` | Light without ray tracing (the settings of crossducking/RAZDA4A-XEN). Don't add a `,floor:` to `VICTORY_AMBIENT`: night skies go black. |
| `VICTORY_STALE_TEXTURES=start` | The old rule that left some flags, walls and clothes black (MTrv0/VIctory-AMD-UM). Leave unset. |
| `GAMESCOPE_MODE=windowed\|borderless\|fullscreen`, `GAMESCOPE_OUT_W/H` | Window mode and size (`run.sh`). Super+F toggles fullscreen. |

Leave these off: `VICTORY_SUBMIT_FRAMES=2` (waxy faces, no gain), `VICTORY_PORT_GUARD` and
`VICTORY_PORT_JOURNAL` (plastic or waxy skin), `VICTORY_PORT_HOME` (very slow under Wine).

## Known problems

- With frame generation, while moving, the screen's edges and fast near objects flicker: the base frame rate (~17 fps) is far below the ~60 fps frame generation is made for, so the generated frames guess wrong where things move far. Still images are clean. Map markers, being part of the UI layer, move at the base rate.
- ~330 draws a frame are dropped (the ray-tracing builder is off), so indoor light is an approximation.
- Loading the world takes over 40 s; memory grows with play time.
- The CPU sets the frame: one submitting thread, much of it Wine's cost of noticing the title's memory writes.

## Debugging

Capturing a frame and replaying it to find a picture fault: [replay.md](replay.md). Commands and
debugging switches: [README-fxceless.md](README-fxceless.md); each `VICTORY_...` switch is described
where the code reads it. The history of what was tried and measured is in this file's git history.

## Where the changes came from

The commits name their sources. The main ones: avatarmanz/VIctory-7900XTX and Belobug/VIctory-Belobug-AMD
(the base and RX 9070 XT fixes), avatarmanz/VIctory-7900XTX#200 (Wine's missing memory query), #437 (plant
pop-in), #438 (batched clamps), Belobug/VIctory-Belobug-AMD#477 and #546, maren-halv/VIctory-Fast `79a92c7`
and `7da8eb5`, Slaktarn700/SlaktarnVI `e5b0dc81` (frame generation), MTrv0/VIctory-AMD-UM `0074263a`
(stale texture slots), HansKristian-Work/dxil-spirv `694cc674`.
