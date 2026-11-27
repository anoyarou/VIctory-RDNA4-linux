# Patches for the projects under VIctory (Linux, Wine)

`git format-patch` files, each under the license of the project it patches. Apply with `git am`.

| Dir | Project | Base | What |
|-----|---------|------|------|
| `wine/` | Proton Experimental's Wine (LGPL-2.1-or-later), with the view-tree wineserver patch | Proton Experimental `wine` as of 2026-11 | `0001`: wineserver makes section memfds sealable; win32u imports a section handle into Vulkan as a dma-buf through `/dev/udmabuf` (after LeanWasTaken/VIctory-bc250 `a25f6ad`, `9b6b938`). `0002`: enables `VK_EXT_external_memory_dma_buf` on the host device, without which `0001` never runs. |
| `vkd3d-proton/` | vkd3d-proton (LGPL-2.1-or-later) | `4522cc1a` (3.1) | LeanWasTaken/VIctory-bc250 `e691b2d`, unchanged: `OpenExistingHeapFromFileMapping` imports the section as the heap's memory. |

Build only `server/wineserver` and `dlls/win32u/win32u.so` (configure **with** Vulkan) and `d3d12core.dll`, and
replace those files in a copy of Proton and beside `victory.exe`. Needs `/dev/udmabuf` readable and writable by
the user. See `docs/linux-wine.md`, "Port 1 as a dma-buf".
