#!/usr/bin/env bash
#
# Compare nera vs BEAM performance on binary benchmarks.
# Runs each benchmark on both runtimes, prints a side-by-side table.
# Shows both total wall-clock time (includes startup) and internal
# timing measured via erlang:monotonic_time/1 inside the benchmark.
#
# Usage:
#   ./benchmarks/compare_benchmarks.sh
#
# Prerequisites:
#   - nera: cargo build --release
#   - BEAM: erl and erlc in PATH
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
NERA="$PROJECT_DIR/target/release/nera"
TMP_DIR="${TMPDIR:-/tmp}/nera_compare_$$"

# Colors
CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
DIM='\033[2m'
RESET='\033[0m'

cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT
mkdir -p "$TMP_DIR/nera" "$TMP_DIR/beam"

# --- Prerequisites ---

HAS_NERA=false
HAS_BEAM=false

if [ -x "$NERA" ]; then
    HAS_NERA=true
else
    echo -e "${YELLOW}Warning:${RESET} nera not found at $NERA (run 'cargo build --release')"
fi

if command -v erl >/dev/null 2>&1 && command -v erlc >/dev/null 2>&1; then
    HAS_BEAM=true
    ERL_VERSION=$(erl -noshell -eval 'io:format("~s", [erlang:system_info(otp_release)]), halt().' 2>/dev/null || echo "?")
else
    echo -e "${YELLOW}Warning:${RESET} erl/erlc not found in PATH (install Erlang/OTP)"
fi

if [ "$HAS_NERA" = false ] && [ "$HAS_BEAM" = false ]; then
    echo "Error: neither nera nor BEAM is available."
    exit 1
fi

# --- OS detection for /usr/bin/time ---

if [[ "$(uname)" == "Darwin" ]]; then
    TIME_CMD="/usr/bin/time -l"
else
    TIME_CMD="/usr/bin/time -v"
fi

# --- Helpers ---

extract_wall_time() {
    local output="$1"
    if [[ "$(uname)" == "Darwin" ]]; then
        echo "$output" | grep -o '[0-9]*\.[0-9]* real' | head -1 | grep -o '[0-9]*\.[0-9]*' || echo "N/A"
    else
        echo "$output" | grep "wall clock" | grep -o '[0-9:]*\.[0-9]*' || echo "N/A"
    fi
}

extract_rss_mb() {
    local output="$1"
    if [[ "$(uname)" == "Darwin" ]]; then
        local rss_bytes
        rss_bytes=$(echo "$output" | grep "maximum resident set size" | grep -o '[0-9]*' | head -1 || echo "0")
        if [ "$rss_bytes" -gt 0 ] 2>/dev/null; then
            echo "$((rss_bytes / 1024 / 1024))"
        else
            echo "N/A"
        fi
    else
        local rss_kb
        rss_kb=$(echo "$output" | grep "Maximum resident set size" | grep -o '[0-9]*' || echo "0")
        if [ "$rss_kb" -gt 0 ] 2>/dev/null; then
            echo "$((rss_kb / 1024))"
        else
            echo "N/A"
        fi
    fi
}

extract_internal_ms() {
    local output="$1"
    # Extract the time_ms:NNN line printed by the benchmark
    echo "$output" | grep -o 'time_ms:[0-9]*' | head -1 | grep -o '[0-9]*' || echo "N/A"
}

# --- Run benchmark on nera ---

run_nera() {
    local name="$1"
    local erl_file="$SCRIPT_DIR/${name}.erl"
    local binary="$TMP_DIR/nera/${name}"

    # Run `nera build` from $TMP_DIR so it doesn't auto-discover
    # unrelated .erl files under the nera repo's `tests/` subtree
    # (e.g. tests/integration/poolboy/.nera/deps/poolboy/test/*.erl,
    # which eunit-include headers nera doesn't parse). The nera CLI
    # currently walks `CWD/tests/` even when explicit files are
    # passed — documented gotcha, may be fixed upstream later.
    ( cd "$TMP_DIR/nera" && "$NERA" build "$erl_file" -o "$binary" -e "${name}:start" ) 2>/dev/null
    local output
    output=$( { $TIME_CMD "$binary" ; } 2>&1 ) || true

    NERA_WALL=$(extract_wall_time "$output")
    NERA_RSS=$(extract_rss_mb "$output")
    NERA_INTERNAL=$(extract_internal_ms "$output")
}

# --- Run benchmark on BEAM ---

run_beam() {
    local name="$1"
    local erl_file="$SCRIPT_DIR/${name}.erl"

    erlc -o "$TMP_DIR/beam" "$erl_file" 2>/dev/null
    local output
    output=$( { $TIME_CMD erl -noshell -pa "$TMP_DIR/beam" -s "$name" start -s init stop ; } 2>&1 ) || true

    BEAM_WALL=$(extract_wall_time "$output")
    BEAM_RSS=$(extract_rss_mb "$output")
    BEAM_INTERNAL=$(extract_internal_ms "$output")
}

# --- Main ---

echo ""
echo -e "${BOLD}nera vs BEAM — Benchmarks${RESET}"
if [ "$HAS_BEAM" = true ]; then
    echo -e "${DIM}BEAM: OTP $ERL_VERSION${RESET}"
fi
echo "════════════════════════════════════════════════════════════"
echo ""

BENCHMARKS=("bench_compute" "bench_tailcall" "bench_binary_send" "bench_binary_fanout" "bench_binary_subbinary" "bench_tcp_ping_pong" "bench_tcp_throughput" "bench_tcp_connections" "bench_process_lifecycle")
DESCRIPTIONS=("pure compute loops" "tail-recursive loops" "100K msgs × 10KB" "1000 procs × 100KB" "100K sub-binary × 100KB" "50K round-trips × 1KB" "100MB bulk send" "100 concurrent conns" "spawn/hibernate/kill")

for i in "${!BENCHMARKS[@]}"; do
    name="${BENCHMARKS[$i]}"
    desc="${DESCRIPTIONS[$i]}"
    erl_file="$SCRIPT_DIR/${name}.erl"

    if [ ! -f "$erl_file" ]; then
        echo -e "  ${YELLOW}SKIP${RESET}  $name (file not found)"
        continue
    fi

    echo -e "${CYAN}${name}${RESET} ${DIM}(${desc})${RESET}"

    NERA_WALL="—"; NERA_RSS="—"; NERA_INTERNAL="—"
    BEAM_WALL="—"; BEAM_RSS="—"; BEAM_INTERNAL="—"

    if [ "$HAS_NERA" = true ]; then
        run_nera "$name"
        echo -e "  nera:  ${GREEN}${NERA_INTERNAL} ms${RESET} internal   ${DIM}${NERA_WALL}s total${RESET}   ${NERA_RSS} MB RSS"
    fi

    if [ "$HAS_BEAM" = true ]; then
        run_beam "$name"
        echo -e "  BEAM:  ${GREEN}${BEAM_INTERNAL} ms${RESET} internal   ${DIM}${BEAM_WALL}s total${RESET}   ${BEAM_RSS} MB RSS"
    fi

    echo ""
done

echo "════════════════════════════════════════════════════════════"
echo -e "${DIM}internal = benchmark work only (erlang:monotonic_time)${RESET}"
echo -e "${DIM}total    = wall-clock including startup (/usr/bin/time)${RESET}"
echo -e "${BOLD}Done.${RESET}"
