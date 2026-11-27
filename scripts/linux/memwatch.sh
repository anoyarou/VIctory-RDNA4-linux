#!/usr/bin/env bash
# Samples memory every 5 s into <out> ("t_s avail_MiB swapfree_MiB victory_rss_MiB"). Stops the game's
# Wine processes if available memory plus free swap falls under MIN_FREE_MIB (default 3072), and ends once
# the game has been gone for 30 s. Usage: memwatch.sh <out> [<wine prefix>]
OUT="${1:?usage: memwatch.sh <out> [<wine prefix>]}"; PFX="${2:-${WINEPREFIX:-}}"
MIN="${MIN_FREE_MIB:-3072}"; start=$(date +%s); seen=0; gone=0
echo "t_s avail_MiB swapfree_MiB victory_rss_MiB" > "$OUT"
stop_game() {
    if [ -n "$PFX" ]; then
        for p in $(grep -lsz "^WINEPREFIX=$PFX\$" /proc/[0-9]*/environ | cut -d/ -f3); do kill -9 "$p" 2>/dev/null; done
    fi
    pkill -9 -f 'victory.exe run' 2>/dev/null
}
while true; do
    a=$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo)
    s=$(awk '/SwapFree/{print int($2/1024)}' /proc/meminfo)
    r=$(pgrep -f 'victory.exe run' | xargs -r ps -o rss= -p | awk '{t+=$1} END{print int(t/1024)}')
    echo "$(( $(date +%s) - start )) $a $s ${r:-0}" >> "$OUT"
    if [ "${r:-0}" -gt 0 ]; then seen=1; gone=0; elif [ "$seen" = 1 ]; then gone=$((gone + 5)); fi
    if [ $((a + s)) -lt "$MIN" ] && [ "$seen" = 1 ]; then
        echo "GUARD: under ${MIN} MiB available, game stopped" >> "$OUT"; stop_game; exit 0
    fi
    [ "$gone" -ge 30 ] && { echo "game gone, watcher ends" >> "$OUT"; exit 0; }
    sleep 5
done
