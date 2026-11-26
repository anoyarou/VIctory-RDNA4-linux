# Running under Wine/Proton on Linux (notes for this branch)

This branch, `rx9070xt-wine`, is Belobug/VIctory-Belobug-AMD's `rx9070xt` branch (the RX 7900 XTX
fork, avatarmanz/VIctory-7900XTX, plus Belobug `81ebb7b` for the RX 9070 XT) with what our Wine runs
needed ported from our earlier branch `speed-trial` (on Belobug `main`). That branch's
`docs/linux-wine.md` has the measurements of 32 runs; this one is **untested under Wine** so far.

## Setup (carried over from `speed-trial`, not yet verified on this branch)

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
  `speed-trial` used 10-11 GB under Wine. Run with a memory guard on 32 GB.

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
