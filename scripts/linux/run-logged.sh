#!/usr/bin/env bash
# run.sh with a log: stops what is left of an earlier run in the same prefix, starts memwatch.sh, and writes
# the console with seconds since the start to <LOG_DIR>/<name>-<date>.console (.mem: memory every 5 s).
#
#   TITLE_EXE=<path to the title executable> scripts/linux/run-logged.sh <name> [extra victory args]
#
# Same environment as run.sh, plus LOG_DIR [$VICTORY_DIR/logs]. Diagnostics builds also get the panel
# (VICTORY_PANEL=1), crash breakpoints, and VICTORY_STEP_OVER for the intro's pool stop (Marvisak/VIctory#698,
# #2644); set any of them to override.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
name="${1:?usage: run-logged.sh <name> [extra victory args]}"; shift
VICTORY_DIR="${VICTORY_DIR:-$HOME/Games/victory}"
export WINEPREFIX="${WINEPREFIX:-$HOME/Games/victory-prefix}"
LOG_DIR="${LOG_DIR:-$VICTORY_DIR/logs}"
mkdir -p "$LOG_DIR"

# Whatever is left of an earlier run in this prefix (killall would miss victory.exe and winedevice.exe).
for p in $(grep -lsz "^WINEPREFIX=$WINEPREFIX\$" /proc/[0-9]*/environ | cut -d/ -f3); do kill -9 "$p" 2>/dev/null; done
free -h | sed -n 1,3p

base="$LOG_DIR/$name-$(date +%Y%m%d-%H%M%S)"
echo "$base" > "$LOG_DIR/current"
nohup "$here/memwatch.sh" "$base.mem" "$WINEPREFIX" >/dev/null 2>&1 &
echo "[logged] $base.console"
export VICTORY_PANEL="${VICTORY_PANEL:-1}" VICTORY_CRASH_BREAKPOINTS="${VICTORY_CRASH_BREAKPOINTS:-1}"
export VICTORY_STEP_OVER="${VICTORY_STEP_OVER:-63966f,5149aec}"
"$here/run.sh" "$@" 2>&1 | awk -v s="$(date +%s)" '{printf "%5ds %s\n", systime()-s, $0; fflush()}' > "$base.console"
