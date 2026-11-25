# Running under Wine/Proton on Linux (notes for this branch)

VIctory is a Windows program. On Linux it runs under Wine, here Valve's Proton Experimental
(`experimental-11.0-20261112`) with its vkd3d-proton, on an RX 9070 XT (RADV). These notes are what
worked on that one machine, measured over 32 runs; they are not a supported platform.

## Setup

- **Native Direct3D 12 and Microsoft's compiler:**
  `WINEDLLOVERRIDES="d3d12,d3d12core,dxgi,d3d11,d3d10core=n,b;d3dcompiler_47=n"`. Put a 64-bit
  Microsoft `d3dcompiler_47.dll` next to `victory.exe`. Wine's own refuses the runtime's
  indirect-argument clamp (`E5005`), the log says `no argument clamp: indirect execution dropped`, and
  most of the world is not drawn (Belobug #183).
- **`VKD3D_CONFIG=ignore_shared_fence,descriptor_heap`.** Without `descriptor_heap`, vkd3d-proton caps
  the view heap at 1,000,000 descriptors (`host view heap capped` in the log). On RADV that gave
  `SQC (data)` GPU page faults and a ring reset once the world loaded. It needs ReBAR.
- **This fork's Wine PRs** #53 (title memory copied, not used in place), #69 (no sparse copies) and #124
  (the pipeline library kept between runs) are merged here.
- **Building from Linux:** `cargo xwin build --profile diag --target x86_64-pc-windows-msvc -p victory`
  ([cargo-xwin](https://github.com/rust-cross/cargo-xwin)). Test binaries (`cargo xwin test ... --no-run`)
  run under the same Proton's `wine`. Under Wine, 10 GPU and shader-compiler tests of `victory-gpu` fail
  on this branch as on its base. The `watch::` journal tests can disturb each other in a parallel run
  (one global journal); `--test-threads=1` runs them apart.

## wineserver

Loading the world is bound by wineserver under stock Wine: it keeps a process's views in a list and
walks it on every map and unmap, and the title remaps its pools one 64 KiB page at a time
(Marvisak/VIctory#834, #400). A wineserver built from Valve's `experimental_11.0` source with the
view-tree patch (Wine bug 60868, CachyOS/wine-cachyos#50), dropped into a copy of Proton
Experimental, measured: wineserver at ~6% of a core while loading against 61-72% stock, loading much
faster, and a scene that never finished loading complete. Standing still in a loaded scene,
wineserver is near idle either way (4-6%). Only `server/` needs building; regenerate the protocol
headers with `tools/make_requests` first (Valve's tree carries stale ones).

## Switches this branch adds or changes

| Switch | Default | What it does |
|---|---|---|
| `VICTORY_STEP_OVER=<hex offsets>` | off | Diagnostics builds: steps over the title's breakpoints at those image offsets (Marvisak/VIctory#2644). `63966f,5149aec` gets past the intro's pool stop (Marvisak/VIctory#698); the title then reaches gameplay and saves. |
| `VICTORY_MERGE_LISTS=1` | off | A submission's lists go to the host in one `ExecuteCommandLists` (1,586 to 136 calls a frame here). Split again for 10 s after a submission spans 250 ms. |
| `VICTORY_MERGE_FILLS=0` | on | Consecutive fills that continue one another become one copy. None did here; harmless. |
| `VICTORY_BESIDE=1` | off | Copy-back writes view memory through a view of ours instead of opening the guards. Removed guard opening, but noting the writes cost as much again; the frame was slightly slower. |
| `VICTORY_MEMORY_LOG_SECS=0` | 5 s | The memory report walks the whole address space with `VirtualQuery` every 5 s, also in release builds. Under Wine that holds the memory lock others need; it did not change the frame here. |

Under Wine only, and without a switch: write watching is asked once per range per submission and a
whole allocation at a time, from a cache (after Slaktarn700/VIctory `4f9aa10c`, `27eefe4e`).

## What it reaches, and why it stays slow

From a save: gameplay at about 2 fps standing still (450-570 ms a frame), with the GPU busy only
~55 ms of it; 10-11 GB for the process. No world freeze since the copy-back race fix
(Marvisak/VIctory#721; one-commit control/candidate pair and 23 runs since). Linux `perf` and
`perf trace` show no thread saturated. The busiest title thread spends ~76% of its time in the kernel
on system calls through Wine (futex, rt_sigprocmask, ioctl, getrusage), and copy-back (115-145 ms a
frame) is half guard opening and writes into just-opened pages. `VICTORY_PROFILE` does not work under
Wine (one sample per 20 s, more memory); sample with Linux `perf` on the game process instead.
