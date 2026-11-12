#!/usr/bin/env bash
#
# Benchmark runner for nera binary performance.
# Compiles and runs each benchmark, capturing wall-clock time and peak RSS.
#
# Usage:
#   ./benchmarks/run_benchmarks.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
NERA="$PROJECT_DIR/target/release/nera"
TMP_DIR="${TMPDIR:-/tmp}/nera_bench_$$"

# Colors
CYAN='\033[0;36m'
GREEN='\033[0;32m'
BOLD='\033[1m'
RESET='\033[0m'

cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT
mkdir -p "$TMP_DIR"

# Check that nera is built
if [ ! -x "$NERA" ]; then
    echo "Error: nera not found at $NERA"
    echo "Run 'cargo build --release' first."
    exit 1
fi

# Detect OS for /usr/bin/time flags
if [[ "$(uname)" == "Darwin" ]]; then
    TIME_CMD="/usr/bin/time -l"
    RSS_LABEL="maximum resident set size"
else
    TIME_CMD="/usr/bin/time -v"
    RSS_LABEL="Maximum resident set size"
fi

echo -e "${BOLD}nera Binary Benchmarks${RESET}"
echo "═══════════════════════════════════════════════════"
echo ""

run_bench() {
    local name="$1"
    local erl_file="$SCRIPT_DIR/${name}.erl"
    local module="${name}"
    local binary="$TMP_DIR/${name}"

    if [ ! -f "$erl_file" ]; then
        echo "  SKIP  $name (file not found)"
        return
    fi

    # Compile
    echo -e "${CYAN}Compiling${RESET} $name..."
    "$NERA" build "$erl_file" -o "$binary" -e "${module}:start" 2>/dev/null

    # Run with /usr/bin/time to capture RSS and wall time
    echo -e "${CYAN}Running${RESET}  $name..."
    local time_output
    time_output=$( { $TIME_CMD "$binary" ; } 2>&1 ) || true

    # Extract the benchmark's own output (first lines before time output)
    local bench_output
    bench_output=$(echo "$time_output" | head -1)

    # Extract wall-clock time (real time)
    local real_time
    if [[ "$(uname)" == "Darwin" ]]; then
        # macOS: time output has "N.NN real" on a line
        real_time=$(echo "$time_output" | grep -o '[0-9]*\.[0-9]* real' | head -1 || echo "N/A")
    else
        # Linux: "Elapsed (wall clock) time: H:MM:SS"
        real_time=$(echo "$time_output" | grep "wall clock" | grep -o '[0-9:]*\.[0-9]*' || echo "N/A")
    fi

    # Extract peak RSS
    local peak_rss
    if [[ "$(uname)" == "Darwin" ]]; then
        # macOS: RSS in bytes
        local rss_bytes
        rss_bytes=$(echo "$time_output" | grep "$RSS_LABEL" | grep -o '[0-9]*' | head -1 || echo "0")
        if [ "$rss_bytes" -gt 0 ] 2>/dev/null; then
            peak_rss="$((rss_bytes / 1024 / 1024)) MB ($rss_bytes bytes)"
        else
            peak_rss="N/A"
        fi
    else
        # Linux: RSS in KB
        local rss_kb
        rss_kb=$(echo "$time_output" | grep "$RSS_LABEL" | grep -o '[0-9]*' || echo "0")
        if [ "$rss_kb" -gt 0 ] 2>/dev/null; then
            peak_rss="$((rss_kb / 1024)) MB ($rss_kb KB)"
        else
            peak_rss="N/A"
        fi
    fi

    # Extract internal timing (time_ms:NNN from benchmark output)
    local internal_ms
    internal_ms=$(echo "$time_output" | grep -o 'time_ms:[0-9]*' | head -1 | grep -o '[0-9]*' || echo "N/A")

    echo -e "  ${GREEN}$bench_output${RESET}"
    echo "  Internal:  ${internal_ms} ms"
    echo "  Wall time: $real_time"
    echo "  Peak RSS:  $peak_rss"
    echo ""
}

run_bench "bench_compute"
run_bench "bench_tailcall"
run_bench "bench_binary_send"
run_bench "bench_binary_fanout"
run_bench "bench_binary_subbinary"
run_bench "bench_tcp_ping_pong"
run_bench "bench_tcp_throughput"
run_bench "bench_tcp_connections"
run_bench "bench_process_lifecycle"
run_bench "bench_selective_receive"

echo "═══════════════════════════════════════════════════"
echo -e "${BOLD}Done.${RESET}"
