#!/usr/bin/env bash
#
# Local process benchmark: nera vs BEAM
#
# Measures, on this computer, the wall time from exec to exit and the peak
# resident memory (RSS) of the demo handler, built natively with nera and
# run with `erl -noshell`.
#
# These are not AWS Lambda cold starts: a Lambda cold start also creates the
# execution environment and starts the bootstrap. Peak RSS is not billed
# memory: Lambda bills the memory you configure (128 MB minimum) for the
# billed duration. macOS /usr/bin/time reports wall time in 10 ms steps.
# See docs/SERVERLESS_ERLANG.md.
#
# Usage:
#   ./demo/serverless/bench_cold_start.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
NERA="$PROJECT_DIR/target/release/nera"
TMP_DIR="${TMPDIR:-/tmp}/nera_lambda_bench_$$"
RUNS=10

# Colors
CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
RESET='\033[0m'

cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT
mkdir -p "$TMP_DIR"

info() { echo -e "${CYAN}==>${RESET} ${BOLD}$*${RESET}"; }
ok()   { echo -e "${GREEN}==>${RESET} ${BOLD}$*${RESET}"; }

# --- Check prerequisites -----------------------------------------------------
[ -x "$NERA" ] || { echo "Build nera first: cargo build --release --locked"; exit 1; }
HAS_BEAM=true
command -v erl >/dev/null 2>&1 || HAS_BEAM=false

# --- Build nera handler ------------------------------------------------------
# Build from this folder: nera build writes its temporary object files in
# the current folder.
info "Compiling handler with nera..."
(cd "$SCRIPT_DIR" && "$NERA" build handler.erl -o "$TMP_DIR/handler_nera" -e handler:start 2>/dev/null)

# --- Build BEAM handler (if erl available) -----------------------------------
if $HAS_BEAM; then
    info "Compiling handler with BEAM..."
    erlc -o "$TMP_DIR" "$SCRIPT_DIR/handler.erl" 2>/dev/null

    # Create escript wrapper for fair comparison (single command cold start)
    cat > "$TMP_DIR/run_beam.sh" << 'BEAMEOF'
#!/bin/sh
cd "$(dirname "$0")"
erl -noshell -pa . -eval "handler:start(), halt()."
BEAMEOF
    chmod +x "$TMP_DIR/run_beam.sh"
fi

# --- Measure function --------------------------------------------------------
# Returns: wall_time_ms peak_rss_kb
measure() {
    local cmd="$1"
    local dir="$2"

    if [[ "$(uname)" == "Darwin" ]]; then
        # macOS: use gtime (GNU time) if available, fallback to /usr/bin/time
        TIME_CMD="/usr/bin/time -l"
        local output
        output=$( { $TIME_CMD "$cmd" > /dev/null; } 2>&1 )
        local wall_ms rss_kb
        # macOS /usr/bin/time outputs seconds as "N.NN real"
        wall_ms=$(echo "$output" | grep 'real' | awk '{printf "%.0f", $1 * 1000}')
        # macOS reports RSS in bytes
        rss_kb=$(echo "$output" | grep 'maximum resident' | awk '{printf "%.0f", $1 / 1024}')
        echo "$wall_ms $rss_kb"
    else
        # Linux: GNU time
        local output
        output=$( { /usr/bin/time -v "$cmd" > /dev/null; } 2>&1 )
        local wall_ms rss_kb
        wall_ms=$(echo "$output" | grep 'Elapsed' | sed 's/.*: //' | awk -F: '{if(NF==3) printf "%.0f",($1*3600+$2*60+$3)*1000; else if(NF==2) printf "%.0f",($1*60+$2)*1000}')
        rss_kb=$(echo "$output" | grep 'Maximum resident' | awk '{print $NF}')
        echo "$wall_ms $rss_kb"
    fi
}

# --- Run benchmarks ----------------------------------------------------------
echo ""
echo -e "${BOLD}Local Process Benchmark (not a Lambda measurement)${RESET}"
echo -e "${BOLD}==================================================${RESET}"
echo ""
echo "Each measurement = full local process lifecycle (exec → compute → exit)"
echo "Runs: $RUNS"
echo ""

# nera
info "Benchmarking nera ($RUNS runs)..."
NERA_TIMES=()
NERA_RSS=()
for i in $(seq 1 $RUNS); do
    result=$(measure "$TMP_DIR/handler_nera" "$TMP_DIR")
    NERA_TIMES+=($(echo "$result" | awk '{print $1}'))
    NERA_RSS+=($(echo "$result" | awk '{print $2}'))
done

# Compute averages
nera_avg_time=0
nera_avg_rss=0
for t in "${NERA_TIMES[@]}"; do nera_avg_time=$((nera_avg_time + t)); done
for r in "${NERA_RSS[@]}"; do nera_avg_rss=$((nera_avg_rss + r)); done
nera_avg_time=$((nera_avg_time / RUNS))
nera_avg_rss=$((nera_avg_rss / RUNS))
nera_avg_rss_mb=$(echo "scale=1; $nera_avg_rss / 1024" | bc)

echo ""
echo -e "${GREEN}nera (native binary)${RESET}"
echo "  Exec to exit: ${nera_avg_time}ms avg"
echo "  Peak RSS:     ${nera_avg_rss_mb}MB avg"
echo "  Binary size:  $(du -h "$TMP_DIR/handler_nera" | awk '{print $1}')"

# BEAM
if $HAS_BEAM; then
    info "Benchmarking BEAM ($RUNS runs)..."
    BEAM_TIMES=()
    BEAM_RSS=()
    for i in $(seq 1 $RUNS); do
        result=$(measure "$TMP_DIR/run_beam.sh" "$TMP_DIR")
        BEAM_TIMES+=($(echo "$result" | awk '{print $1}'))
        BEAM_RSS+=($(echo "$result" | awk '{print $2}'))
    done

    beam_avg_time=0
    beam_avg_rss=0
    for t in "${BEAM_TIMES[@]}"; do beam_avg_time=$((beam_avg_time + t)); done
    for r in "${BEAM_RSS[@]}"; do beam_avg_rss=$((beam_avg_rss + r)); done
    beam_avg_time=$((beam_avg_time / RUNS))
    beam_avg_rss=$((beam_avg_rss / RUNS))
    beam_avg_rss_mb=$(echo "scale=1; $beam_avg_rss / 1024" | bc)

    echo ""
    echo -e "${YELLOW}BEAM (erl -noshell)${RESET}"
    echo "  Exec to exit: ${beam_avg_time}ms avg"
    echo "  Peak RSS:     ${beam_avg_rss_mb}MB avg"

    # Comparison
    echo ""
    echo -e "${BOLD}Local comparison (BEAM / nera)${RESET}"
    if [ "$nera_avg_time" -gt 0 ]; then
        time_ratio=$(echo "scale=1; $beam_avg_time / $nera_avg_time" | bc 2>/dev/null || echo "N/A")
        echo "  Exec to exit: ${time_ratio}x"
    fi
    if [ "$nera_avg_rss" -gt 0 ]; then
        mem_ratio=$(echo "scale=1; $beam_avg_rss / $nera_avg_rss" | bc 2>/dev/null || echo "N/A")
        echo "  Peak RSS:     ${mem_ratio}x"
    fi
    echo ""
    echo "These are local process measurements, not Lambda cold starts or costs."
    echo "Lambda bills configured memory (128 MB minimum) for the billed duration."
else
    echo ""
    echo "(BEAM not installed — install Erlang/OTP for comparison)"
fi
echo ""
