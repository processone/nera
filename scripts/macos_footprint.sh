#!/usr/bin/env bash
# macOS memory measurement: phys_footprint vs RSS
#
# macOS RSS includes pages marked MADV_FREE (freed by jemalloc but not
# reclaimed by the kernel). phys_footprint excludes them — it shows the
# actual physical memory the process needs.
#
# Usage: ./scripts/macos_footprint.sh <pid>
#        ./scripts/macos_footprint.sh <pid> --watch   (poll every 2s)

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <pid> [--watch]"
    exit 1
fi

PID=$1
WATCH=${2:-""}

get_footprint() {
    local pid=$1
    # Use memory_pressure per-process footprint (available on macOS 12+)
    # Fallback to footprint command or vmmap
    if command -v footprint &>/dev/null; then
        footprint -j "$pid" 2>/dev/null | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    for entry in data:
        fp = entry.get('phys_footprint', 0)
        print(f'phys_footprint: {fp / 1048576:.2f} MB')
except:
    print('(footprint parse failed)')
" 2>/dev/null && return
    fi

    # Fallback: use ps for RSS + vmmap for dirty/swapped
    rss_kb=$(ps -o rss= -p "$pid" 2>/dev/null || echo 0)
    rss_mb=$(echo "scale=2; $rss_kb / 1024" | bc)

    # Get dirty+compressed from vmmap summary
    if vmmap_out=$(vmmap --summary "$pid" 2>/dev/null); then
        dirty=$(echo "$vmmap_out" | grep "^TOTAL" | awk '{print $NF}')
        echo "RSS: ${rss_mb} MB  |  vmmap TOTAL dirty: ${dirty:-?}"
    else
        echo "RSS: ${rss_mb} MB  (vmmap not available, run as root or same user)"
    fi
}

if [[ "$WATCH" == "--watch" ]]; then
    echo "Watching PID $PID every 2s (Ctrl+C to stop)"
    while kill -0 "$PID" 2>/dev/null; do
        ts=$(date +%H:%M:%S)
        echo -n "$ts  "
        get_footprint "$PID"
        sleep 2
    done
    echo "Process $PID exited."
else
    get_footprint "$PID"
fi
