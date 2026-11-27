#!/usr/bin/env bash
# Runs victory.exe under Proton's Wine inside gamescope, with the settings of settings.sh.
#
#   TITLE_EXE=<path to the title executable of your copy> scripts/linux/run.sh [extra victory args]
#
# Environment (defaults in brackets):
#   TITLE_EXE      the full path to the title executable in your copy of the title (required)
#   VICTORY_DIR    the folder holding victory.exe, Microsoft's 64-bit d3dcompiler_47.dll and, if you use one,
#                  a rebuilt d3d12core.dll [$HOME/Games/victory]; build-xwin.sh --deploy-dir puts the exe there
#   PROTON_DIR     a Proton folder [Steam's "Proton - Experimental"]. Use one with the view-tree wineserver
#                  (Wine bug 60868): with the stock one, loading and the frame are about twice as slow here.
#   WINEPREFIX     a Proton/Wine prefix [$HOME/Games/victory-prefix]
#   SETTINGS=0     skip settings.sh
#   GAMESCOPE_W/H  the size the game renders at [1600x900]
#   GAMESCOPE_MODE windowed (a normal, movable window) | borderless | fullscreen [windowed]
#   GAMESCOPE_OUT_W/H  the window or screen size [the render size]; e.g. 2560x1440 with fullscreen
#   GAMESCOPE_FILTER   gamescope's upscaler when the two sizes differ: linear, fsr, nis, nearest [linear]
# Super+F toggles fullscreen in any mode. What each setting is for, measured: docs/linux-wine.md.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

: "${TITLE_EXE:?set TITLE_EXE to the full path of the title executable in your copy}"
VICTORY_DIR="${VICTORY_DIR:-$HOME/Games/victory}"
PROTON_DIR="${PROTON_DIR:-$HOME/.steam/steam/steamapps/common/Proton - Experimental}"
export WINEPREFIX="${WINEPREFIX:-$HOME/Games/victory-prefix}"
WINE_BIN="$PROTON_DIR/files/bin/wine"

[ -x "$WINE_BIN" ] || { echo "No Wine at $WINE_BIN (set PROTON_DIR)" >&2; exit 1; }
[ -f "$VICTORY_DIR/victory.exe" ] || { echo "No victory.exe in $VICTORY_DIR (set VICTORY_DIR)" >&2; exit 1; }
[ -f "$TITLE_EXE" ] || { echo "No title executable at $TITLE_EXE (set TITLE_EXE)" >&2; exit 1; }
[ -f "$VICTORY_DIR/d3dcompiler_47.dll" ] || echo "Warning: no d3dcompiler_47.dll beside victory.exe; indirect draws will be dropped" >&2
command -v gamescope >/dev/null || { echo "gamescope not found" >&2; exit 1; }

[ "${SETTINGS:-1}" = 1 ] && { set -a; . "$here/settings.sh"; set +a; }

export WINEDEBUG="${WINEDEBUG:--all}"
# Native D3D12 (vkd3d-proton) and Microsoft's compiler: Wine's builtin d3dcompiler refuses VIctory's helper
# shaders (E5005), which drops every indirect draw (Belobug/VIctory-Belobug-AMD#183).
export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-d3d12,d3d12core,dxgi,d3d11,d3d10core=n,b;d3dcompiler_47=n;wintab32=d}"
# descriptor_heap lifts vkd3d-proton's view heap cap (needs Resizable BAR); without it RADV page-faults in the world.
export VKD3D_CONFIG="${VKD3D_CONFIG:-ignore_shared_fence,descriptor_heap}"
# Copies on the SDMA queue: copy-back ~38 -> 24-27 ms (avatarmanz/VIctory-7900XTX#111). RADV_EXPERIMENTAL= turns it off.
export RADV_EXPERIMENTAL="${RADV_EXPERIMENTAL-transfer_queue}"
# Controllers through SDL (DualSense and others).
export SDL_JOYSTICK_HIDAPI="${SDL_JOYSTICK_HIDAPI:-1}" PROTON_DISABLE_HIDRAW="${PROTON_DISABLE_HIDRAW:-1}"

GAMESCOPE_W="${GAMESCOPE_W:-1600}" GAMESCOPE_H="${GAMESCOPE_H:-900}"
GAMESCOPE_OUT_W="${GAMESCOPE_OUT_W:-$GAMESCOPE_W}" GAMESCOPE_OUT_H="${GAMESCOPE_OUT_H:-$GAMESCOPE_H}"
GAMESCOPE_MODE="${GAMESCOPE_MODE:-windowed}"
gs=(-w "$GAMESCOPE_W" -h "$GAMESCOPE_H" -W "$GAMESCOPE_OUT_W" -H "$GAMESCOPE_OUT_H" -S fit)
case "$GAMESCOPE_MODE" in
    windowed) ;;
    borderless) gs+=(-b) ;;
    fullscreen) gs+=(-f) ;;
    *) echo "GAMESCOPE_MODE must be windowed, borderless or fullscreen" >&2; exit 2 ;;
esac
[ -n "${GAMESCOPE_FILTER:-}" ] && gs+=(-F "$GAMESCOPE_FILTER")

cd "$VICTORY_DIR"
echo "[run] Proton: $PROTON_DIR"
echo "[run] gamescope $GAMESCOPE_MODE: ${gs[*]}"
exec gamescope "${gs[@]}" -- \
    "$WINE_BIN" ./victory.exe run "$TITLE_EXE" --execute --console series-x "$@"
