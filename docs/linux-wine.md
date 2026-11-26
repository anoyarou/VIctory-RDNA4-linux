# Running under Wine/Proton on Linux (notes for this branch)

This branch, `rx9070xt-wine`, is Belobug/VIctory-Belobug-AMD's `rx9070xt` branch (the RX 7900 XTX
fork, avatarmanz/VIctory-7900XTX, plus Belobug `81ebb7b` for the RX 9070 XT) with what our Wine runs
needed ported from our earlier branch `speed-trial` (on Belobug `main`). That branch's
`docs/linux-wine.md` has the measurements of 32 runs. On this branch, runs 33-40 (RX 9070 XT, RADV,
Proton Experimental `experimental-11.0-20261112`, from a save, standing still in the first playable
area) [measured on one machine].

## Setup (verified on this branch unless marked)

- **Native Direct3D 12 and Microsoft's compiler:**
  `WINEDLLOVERRIDES="d3d12,d3d12core,dxgi,d3d11,d3d10core=n,b;d3dcompiler_47=n"`, a 64-bit
  Microsoft `d3dcompiler_47.dll` next to `victory.exe` (Belobug/VIctory-Belobug-AMD#183; #233 and
  #243 propose a DXC fallback instead).
- **`VKD3D_CONFIG=ignore_shared_fence,descriptor_heap`** (needs ReBAR); without `descriptor_heap`
  RADV page-faulted at world load on `speed-trial`.
- **wineserver with the view-tree patch** (Wine bug 60868, CachyOS/wine-cachyos#50) for loading;
  see Marvisak/VIctory#834.
- **Building from Linux:** `cargo xwin build --profile diag --target x86_64-pc-windows-msvc -p victory --features victory/diagnostics`.
- **Memory:** this fork's README reports 40-46 GB private memory on Windows (64 GB recommended).
  Under Wine the game process used 13.6-15.6 GB resident here (`speed-trial`: 10-11 GB); 32 GB with
  swap was enough.
- **vkd3d-proton with dxil-spirv from HansKristian-Work/dxil-spirv `694cc674` on.** With the
  vkd3d-proton of Proton Experimental (and an older local build, dxil-spirv `7c048638`), six of the
  title's ray-traced shadow compute shaders, as this branch translates them, send dxil-spirv into an
  endless recursion (`CFGNode::find_common_dominator` <-> `recompute_immediate_dominator`, from
  `flatten_subgroup_shuffles`). The log shows `victory-pipelines-N has overflowed its stack` about 5 s
  in; Belobug `81ebb7b`'s compiler guard keeps the process alive, but the six pipelines stay "being
  made" for the whole run. A bigger worker stack does not help (tried in `13e730d`, reverted in
  `2d5fa28`). `694cc674` ("Only run fixup passes after structurizer has run") fixes it, found by
  bisecting with the shaders translated natively (`victory_shader::emit::translate`) and dxil-spirv's
  CLI. With master (`b540c8a2`) the six convert, `spirv-val` accepts them, and none of a run's 2,707
  shaders fails. Beqren found the same with vkd3d-proton `71cae09` (avatarmanz/VIctory-7900XTX#111).
  Used here without changing the prefix: vkd3d-proton's `d3d12core.dll` rebuilt with that dxil-spirv,
  placed next to `victory.exe`. With `d3d12core=n,b` Wine loads a native DLL from the program's folder
  before `system32`.
- **The fork author's settings** (`scripts/play.ps1`, not `-Safe`): `VICTORY_THIN=1`,
  `VICTORY_JOIN_PARTS=8`, `VICTORY_UPLOAD_AHEAD=1`, `VICTORY_RELAY=2`, `VICTORY_CARD_STEPS=2` and the
  others listed there. Standing still: **575 ms a frame with this fork's defaults (run 33), 290-301 ms
  with these (runs 34-36)**, while the times the frame line gives for our own work hardly changed.
  Likely `VICTORY_THIN`, which takes the list calls off the title's threads (not tested alone).

## Changes on this branch

| Commit | What |
|---|---|
| `76f8581` | `VICTORY_AMBIENT=<share>,floor:<level>`: the ambient stand-in's estimate, never below beloooo's constant. Indoors without rays the estimate feeds on itself (dark walls send out little), and a room stayed too dark at `0.8`. Used as `0.8,floor:0.6`; no measurable frame cost. |
| `fa54da3` | avatarmanz/VIctory-7900XTX#200 (`1b06b56`, amine23012004, after Kayotic1337 and zocram4cc on avatarmanz/VIctory-7900XTX#59): Wine refuses `NtQueryVirtualMemory` class 7, so no hole was ever known and each was asked about with `VirtualQuery` every frame (13,000-23,000 calls, reported). A private, reserved, `PAGE_NOACCESS` block now counts as a placeholder there. Reported: preparing 38-42 -> 24-28 ms on an RX 9070 XT. Untested here. |

## Where the frame went (runs 34-37), and why it is gone

Standing still at ~290 ms a frame, every frame line said `500.0 ms waiting for pipelines` and
`(6 being made)`: the layer's per-frame pipeline wait (`VICTORY_PIPELINE_WAIT_MS`) was spent on the six
pipelines above, which never arrive. Meanwhile one title thread polled a counter (`mwaitx` and
`Sleep(0)`, image `+0x4fb0bd0`, near the frame limiter of Marvisak/VIctory#2469) and the frame line
showed ~215 ms between the title's submissions. With dxil-spirv `b540c8a2` (run 40): 0 ms waiting for
pipelines, 3.4 ms between submissions, ~64 ms a frame; preparing (~57 ms) is what is left. Beqren saw
the same on avatarmanz/VIctory-7900XTX#111; `VICTORY_PIPELINE_WAIT_MS=0` hides it without the fix.

## Ported from `speed-trial`

| Commit there | Here | What |
|---|---|---|
| `d6acf71` (Belobug/VIctory-Belobug-AMD#124) | `7de928a` | Empty pipeline library from a null blob; vkd3d-proton refuses a pointer with no bytes. |
| `6ef5207` (Marvisak/VIctory#2644) | `5a08439`, `fd00e1e` | `VICTORY_STEP_OVER=63966f,5149aec` (diagnostics builds) steps over the intro's pool stop (Marvisak/VIctory#698). |
| `b66e7c8` | `3780e16` | The title's stall watchdog patched out of the known build (Marvisak/VIctory#630); `VICTORY_KEEP_WATCHDOG=1` keeps it. |

## Not ported, because this base already does it another way

| `speed-trial` | Here instead |
|---|---|
| Belobug/VIctory-Belobug-AMD#53: title memory copied, not used in place | In place only with `VICTORY_INPLACE=1` (off). |
| Belobug/VIctory-Belobug-AMD#69: no sparse copies under Wine | No sparse (reserved) copies of title memory. |
| `b394ba9`, run-only write-back (Marvisak/VIctory#721, #1024) | `Shadow::copy_back_with` writes only the dwords the card changed (`put_changed`). |
| `7da3f69`, memory asked of the kernel once per submission | `VICTORY_ASK_ONCE`, on by default. |
| `e6c7083`, `VICTORY_MERGE_LISTS` | `VICTORY_JOIN_PARTS` / `VICTORY_JOIN_LISTS` (off by default). |
| `2f4762e`, `changed_runs` by words; Belobug/VIctory-Belobug-AMD#123 AVX2 merge | Word and AVX2 compares of its own. |
| `7e67c48`, `c727353`, `VICTORY_BESIDE` | `VICTORY_COPY_BESIDE`, `VICTORY_STORE_BESIDE`. |

Left for later, if a run shows the cost: `e423553` (under Wine, write watching asks about whole
allocations from a cache, after Slaktarn700/VIctory `4f9aa10c`, `27eefe4e`), `7f72875` (copies
remember unmapped title memory), `4fa10ea` (pins per block; this base has no pins list),
`8ea54da` (fill merging; no gain on `speed-trial`), and `speed-trial`'s copy-back timers.

Tests: `victory-game` and `victory-loader` unit tests pass under Proton's `wine`; `load_probe` fails
there because it starts `cargo` from inside Wine (`Invalid handle`), not because of the code.
`victory-gpu`'s test binary printed nothing under Wine here (it needs the host's graphics DLLs set
up), so the ambient change was checked in the game (its shader compiled and ran) rather than by
`ambient::tests`.

Open: the logo videos jump back and forth since this base (runs 33 on; not on `speed-trial`, which
had Belobug's uncapped frame pacing `fa07481`); not looked into yet.
