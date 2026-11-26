# Running under Wine/Proton on Linux (notes for this branch)

This branch, `rx9070xt-wine`, is Belobug/VIctory-Belobug-AMD's `rx9070xt` branch (the RX 7900 XTX
fork, avatarmanz/VIctory-7900XTX, plus Belobug `81ebb7b` for the RX 9070 XT) with what our Wine runs
needed ported from our earlier branch `speed-trial` (on Belobug `main`; archived as the tag
`archive/speed-trial`, where every `speed-trial` commit named below can be found). That tag's
`docs/linux-wine.md` has the measurements of 32 runs. On this branch, runs 33-43 (RX 9070 XT, RADV,
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
- **`VICTORY_FETCH_CHECK=1` with `VICTORY_RELAY=2`** (avatarmanz/VIctory-7900XTX#13, #138; #148 makes it
  the default there). Without it the characters' skin looked oily or flat and the eyes wrong (runs
  40-42): with `RELAY=2` a few fetched pieces of GPU copies are torn. In run 43 the check found 3-5
  of ~970 a frame and fetched them again; skin right, no measurable cost. `RELAY=1` also fixes the
  skin but cost ~25 ms a frame (run 41). `VICTORY_LAND_ECHO=0` (avatarmanz/VIctory-7900XTX#201, for
  missing skin in #59) did not help here and sends GPU-written pages up again (run 42).

## Changes on this branch

| Commit | What |
|---|---|
| `76f8581` | `VICTORY_AMBIENT=<share>,floor:<level>`: the ambient stand-in's estimate, never below beloooo's constant. Indoors without rays the estimate feeds on itself (dark walls send out little), and a room stayed too dark at `0.8`. Used as `0.8,floor:0.6`; no measurable frame cost. |
| `fa54da3` | avatarmanz/VIctory-7900XTX#200 (`1b06b56`, amine23012004, after Kayotic1337 and zocram4cc on avatarmanz/VIctory-7900XTX#59): Wine refuses `NtQueryVirtualMemory` class 7, so no hole was ever known and each was asked about with `VirtualQuery` every frame (13,000-23,000 calls, reported). A private, reserved, `PAGE_NOACCESS` block now counts as a placeholder there. Reported: preparing 38-42 -> 24-28 ms on an RX 9070 XT. Here (run 43, with `FETCH_CHECK=1`, at the bunk, ~3,600 draws): **61-64 ms a frame (16 fps)**, against run 40's 64-68 ms at ~3,150 draws; write-watch asking 27-30 ms a frame against up to 78 ms. |

## Where the frame went (runs 34-37), and why it is gone

Standing still at ~290 ms a frame, every frame line said `500.0 ms waiting for pipelines` and
`(6 being made)`: the layer's per-frame pipeline wait (`VICTORY_PIPELINE_WAIT_MS`) was spent on the six
pipelines above, which never arrive. Meanwhile one title thread polled a counter (`mwaitx` and
`Sleep(0)`, image `+0x4fb0bd0`, near the frame limiter of Marvisak/VIctory#2469) and the frame line
showed ~215 ms between the title's submissions. With dxil-spirv `b540c8a2` (run 40): 0 ms waiting for
pipelines, 3.4 ms between submissions, ~64 ms a frame; preparing (~57 ms) is what is left. Beqren saw
the same on avatarmanz/VIctory-7900XTX#111; `VICTORY_PIPELINE_WAIT_MS=0` hides it without the fix.

## Merged from avatarmanz/VIctory-7900XTX `main` (`aad5ed5`, branch `rx9070xt-wine-am`; run 44)

avatarmanz `main` up to `a9400a6`, 161 commits. Switches worth trying under Wine, all off by default
unless noted:

- `VICTORY_PORT_GUARD=1` (renderer port 6, `cc761bf`): faults and refreshes share the guards' lock,
  view memory is not asked of the kernel. In run 43 the threads waited 64 ms a frame for the guards and
  spent 25 ms changing protections; reported on Windows: guard waits 29 -> ~2 ms, frame 47.2 -> 45.1 ms.
  **Run 44** (at the bunk, ~3,540 draws): guard waits 64 -> 0.2 ms and kernel askings 34 -> 4.9 ms a
  frame, but the frame only 61-64 -> 59-61 ms: those waits were mostly off the frame's critical path.
  New in run 44: the inmates' uniforms look plastic. In run 45 (port 6 off, `AMBIENT_SETTLE` on as
  before) they look normal, so most likely port 6 (not proven: run 45 also merged Belobug's branch).
  avatarmanz/VIctory-7900XTX#270 measured port 6 slower under Wine. **Leave it off.** Also leave
  renderer port 7 (`VICTORY_PORT_JOURNAL`) off: waxy skin under Proton there.
- `VICTORY_PORT_CBV=1` (renderer port 5, `0dec5dc`): constant buffers read in place, ~22,000 fills a
  frame fewer (reported). Not `VICTORY_PORT_CONSTANTS=1`: black world frames on RADV
  (avatarmanz/VIctory-7900XTX#163).
- `VICTORY_PLANES` (two-plane compose, `85a663d`, `0ce46d5`), on by default there. It does **not** stop
  the logo videos jumping (run 44).

## Merged from Belobug/VIctory-Belobug-AMD `rx9070xt` (`ba3cc52`, branch `rx9070xt-wine-am`; run 45)

Belobug rebuilt `rx9070xt` (`a-publier` is the same commit, `cd8dfc0`) on avatarmanz `main` `c9c932c`
(renderer port 7, off) with three commits: `e942896` (the RX 9070 XT fixes and the HUD on that base,
VRAM keep thresholds 85/93 %), `5d6bd84` (changed title textures uploaded on the submitting list's
prelude, not a frame late; Belobug/VIctory-Belobug-AMD#279) and `cd8dfc0` (PC FSR 4.1.2 to 4K,
`VICTORY_FSR_4K`, from DexNom/VIctory `ab34a8b2`; not tried: it asks for a 4.1.2 provider).

**Run 45**, with `RADV_EXPERIMENTAL=transfer_queue` (vkd3d-proton's copy queue on SDMA,
avatarmanz/VIctory-7900XTX#111) and port 6 off [measured on one machine, user's eyes for the picture]:
- the intro logo videos no longer jump (likely `5d6bd84`); uniforms and skin right;
- at the bunk at ~2,400-3,300 draws **57-62 ms a frame**, preparing 50-53, **copy-back 24-27 ms**
  (run 44: 38-39; `transfer_queue` and the merge not separated); draws in view vary much more than
  before (up to ~11,400; 72-75 ms at ~7,000);
- dropped ~330 draws a frame without a host pipeline: expected with the ray-tracing builder off
  (Belobug/VIctory-Belobug-AMD#269);
- game process up to 15.6 GB resident.

Not taken: Marvisak/VIctory#3069 and #3199 (copy-back through a view of our own): this base already
lands beside the guards (`VICTORY_LAND_BESIDE`, on; 764 of 1,319 pieces a frame in run 43).

## Picked up 2026-11-27 (`cacc825`; run 57)

`rx9070xt-wine` is again the one branch: it was fast-forwarded to the former trial branch
`rx9070xt-wine-am`, which only added commits on top of it.
- Serph91P/VIctory-7900XTX@75f6b933: live-quad handling restored when a translation retry
  unwinds (fix-up for avatarmanz/VIctory-7900XTX#354, which this branch carries).
- avatarmanz/VIctory-7900XTX#390 (`a89d3921`): textures the title rewrites in place (TV and phone
  video) looked at every frame, fingerprinted whole; `VICTORY_LIVE_TEXTURES=0` turns it off.
- The save part of Belobug/VIctory-Belobug-AMD@705a2d09: an older save version that cannot be removed
  no longer hides the newest one.
- Belobug/VIctory-Belobug-AMD `a-publier` `4eb6cfd` merged (49 commits since `cd8dfc0`): among them #324
  (a size query is not reused; AMD's compiler crashed on 29 compute pipelines on RDNA4), #354 (port 2
  sync), #343 (`VICTORY_PROFILE` no longer crashes), #336 (FSR 3.1 fallback for `-Fsr4K`). Translator
  version `0x4c00_0005`.

Not taken: Belobug `main`'s sky-following ambient (`705a2d09`, `e48eb83e`, `4f62c2a5`, `c3bd5612`) builds
on sky-finding code this branch's ambient stand-in (avatarmanz's estimate) does not have; its plan
rebuild change has no counterpart here. avatarmanz's `VICTORY_PORT_CONSTANTS_ORDER` works only with
renderer port 1 (off here).

**Run 57** [measured on one machine, user's eyes for the picture]: PlayStation prompts and Settings
picture right; the user found the picture better and the frame rate worse. From a new save (the first
mission done), standing at the aquarium at ~8,000 draws: **164 ms a frame** (120-216), preparing 116,
GPU ~1, copy-back 26, texture refreshes 130 ms a frame over all threads; game process up to 16.4 GiB
resident, 2.6-3.1 GiB of swap used by then, so memory pressure may be part of it. Not compared at the
same spot with an earlier build. Run 57 (like runs 52-56) ran on stock Proton Experimental, not the
view-tree wineserver of the setup above, so its frame time is not comparable with run 45's; the
same spot (a snake terrarium) on the view-tree wineserver, **run 58: 81 ms a frame** (72-91) at
~7,800 draws, preparing 66, copy-back 28, texture refreshes 24 ms a frame: in line with run 45's
72-75 ms at ~7,000 draws. The new commits, #390 included, cost nothing visible here. Untested: TV/phone video.

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

Fixed in run 45: the logo videos jumped back and forth since this base (runs 33-44); gone with Belobug
`5d6bd84` (see above).

Fixed in run 46: mouths stayed shut in dialogue (runs 33-45). `9274523` brings avatarmanz/VIctory-7900XTX#114
(the face network's `_d16` accesses through table views), fxceless/victory-fork#927's half views and
Beqren's raw typed loads (`1711cd1c2`, `VICTORY_RAW_TYPED_LOADS`, on), as bordersky/victory-linux
carries them; on Radeon under Proton #114 alone tears the mouths (Fedalv, comments on #114). Mouths
move [user]; at the bunk at ~7,000 draws 72.5-76 ms a frame against run 45's 72-75 ms [measured]. The
first run translates every shader again (new `TRANSLATOR_VERSION`): ~90 s of slow frames.
