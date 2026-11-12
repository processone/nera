#!/usr/bin/env bash
#
# OneCached Performance Benchmark Harness — Phase 0
#
# 3-way comparison: BEAM / Nera / Rust baseline
# Captures ops/sec, latency, RSS, phys_footprint (macOS), memcached stats.
# Multi-run median reporting for stable baselines.
#
# Usage:
#   ./benchmarks/onecached_bench.sh [OPTIONS]
#
# Options:
#   --engines ENGINES   Comma-separated: beam,nera,rust (default: beam,nera,rust)
#   --runs N            Runs per engine (default: 5)
#   --test-time T       memtier duration in seconds (default: 10)
#   --data-size S       Value size in bytes (default: 256)
#   --ratio R           memtier set:get ratio (default: 1:1)
#   --output-dir DIR    Results directory (auto-generated if omitted)
#   --nera-binary PATH  Nera onecached binary
#   --beam-dir PATH     Directory with compiled .beam files
#   --rust-binary PATH  Rust baseline binary
#   --rust-src PATH     Cargo-build rust baseline from this source dir
#                       (e.g. benchmarks/rust_baseline)
#   --port PORT         default memcached port for engines (default: 11212)
#   --beam-port PORT    BEAM engine port override (default: auto from --beam-src or --port)
#   --nera-port PORT    Nera engine port override (default: --port)
#   --rust-port PORT    Rust engine port override (default: --port)
#   --skip-flush-wait   Skip the 60s post-flush wait (faster, less data)
#
# Prerequisites:
#   - memtier_benchmark in PATH
#   - For BEAM: erl in PATH + compiled .beam files
#   - For Nera: compiled onecached_nera binary
#   - For Rust: compiled rust_memcached_baseline binary
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ─── Defaults ──────────────────────────────────────────────────────────
ENGINES="beam,nera,rust"
RUNS=5
TEST_TIME=10
DATA_SIZE=256
RATIO="1:1"
OUTPUT_DIR=""
NERA_BINARY="/tmp/onecached-bench/onecached_nera"
BEAM_DIR="/tmp/onecached-bench/ebin"
BEAM_SRC=""     # optional: compile .beam files from this source dir
RUST_BINARY="/tmp/onecached-bench/rust_memcached_baseline"
RUST_SRC=""     # optional: cargo-build the rust baseline from this source dir
SKIP_FLUSH_WAIT=false
PORT=11212
BEAM_PORT=""
NERA_PORT=""
RUST_PORT=""
WARMUP_TIME=2
MEMTIER_CLIENTS=50
MEMTIER_THREADS=4
MEMTIER_TIMEOUT=60  # kill memtier if it hangs longer than this (seconds)

# ─── Colors ────────────────────────────────────────────────────────────
BOLD='\033[1m'
DIM='\033[2m'
CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
RESET='\033[0m'

# ─── Parse arguments ──────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --engines)     ENGINES="$2"; shift 2 ;;
        --runs)        RUNS="$2"; shift 2 ;;
        --test-time)   TEST_TIME="$2"; shift 2 ;;
        --data-size)   DATA_SIZE="$2"; shift 2 ;;
        --ratio)       RATIO="$2"; shift 2 ;;
        --output-dir)  OUTPUT_DIR="$2"; shift 2 ;;
        --nera-binary) NERA_BINARY="$2"; shift 2 ;;
        --beam-dir)    BEAM_DIR="$2"; shift 2 ;;
        --beam-src)    BEAM_SRC="$2"; shift 2 ;;
        --rust-binary) RUST_BINARY="$2"; shift 2 ;;
        --rust-src)    RUST_SRC="$2"; shift 2 ;;
        --port)        PORT="$2"; shift 2 ;;
        --beam-port)   BEAM_PORT="$2"; shift 2 ;;
        --nera-port)   NERA_PORT="$2"; shift 2 ;;
        --rust-port)   RUST_PORT="$2"; shift 2 ;;
        --skip-flush-wait) SKIP_FLUSH_WAIT=true; shift ;;
        --help|-h)
            sed -n '2,/^set /p' "$0" | grep '^#' | sed 's/^# \?//'
            exit 0 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

if [ -z "$OUTPUT_DIR" ]; then
    OUTPUT_DIR="/tmp/onecached_bench_$(date +%Y%m%d_%H%M%S)"
fi

detect_beam_src_port() {
    if [ -z "$BEAM_SRC" ]; then
        return 1
    fi
    # onecached's layout has `onecached.hrl` either alongside the .erl
    # files or in a sibling `../include/` directory.
    local hrl
    for hrl in "$BEAM_SRC/onecached.hrl" "$BEAM_SRC/../include/onecached.hrl"; do
        if [ -f "$hrl" ]; then
            sed -nE 's/^-define\(PORT,[[:space:]]*([0-9]+)\)\./\1/p' "$hrl" | head -n 1
            return 0
        fi
    done
    return 1
}

resolve_engine_ports() {
    [ -n "$NERA_PORT" ] || NERA_PORT="$PORT"
    [ -n "$RUST_PORT" ] || RUST_PORT="$PORT"
    if [ -n "$BEAM_PORT" ]; then
        return
    fi
    if [ -n "$BEAM_SRC" ]; then
        local src_port
        src_port=$(detect_beam_src_port || true)
        if [ -n "$src_port" ]; then
            BEAM_PORT="$src_port"
            return
        fi
    fi
    BEAM_PORT="$PORT"
}

port_for_engine() {
    local engine=$1
    case "$engine" in
        beam) echo "$BEAM_PORT" ;;
        nera) echo "$NERA_PORT" ;;
        rust) echo "$RUST_PORT" ;;
        *) echo "$PORT" ;;
    esac
}

# ─── Prerequisites ─────────────────────────────────────────────────────
check_prereqs() {
    local ok=true
    if ! command -v memtier_benchmark &>/dev/null; then
        echo -e "${RED}Error:${RESET} memtier_benchmark not found in PATH"
        ok=false
    fi

    IFS=',' read -ra ENGINE_LIST <<< "$ENGINES"
    for eng in "${ENGINE_LIST[@]}"; do
        case "$eng" in
            beam)
                if ! command -v erl &>/dev/null; then
                    echo -e "${RED}Error:${RESET} erl not found (needed for BEAM engine)"
                    ok=false
                elif [ -z "$BEAM_SRC" ] && [ ! -d "$BEAM_DIR" ]; then
                    echo -e "${RED}Error:${RESET} BEAM ebin directory not found: $BEAM_DIR"
                    ok=false
                fi ;;
            nera)
                if [ ! -x "$NERA_BINARY" ]; then
                    echo -e "${RED}Error:${RESET} Nera binary not found: $NERA_BINARY"
                    ok=false
                fi ;;
            rust)
                # If --rust-src is given, compile_rust_src builds the
                # binary later; don't require it to exist now.
                if [ -z "$RUST_SRC" ] && [ ! -x "$RUST_BINARY" ]; then
                    echo -e "${RED}Error:${RESET} Rust binary not found: $RUST_BINARY"
                    ok=false
                fi ;;
            *)
                echo -e "${RED}Error:${RESET} Unknown engine: $eng"
                ok=false ;;
        esac
    done

    if [ -n "$BEAM_SRC" ]; then
        if [ ! -d "$BEAM_SRC" ]; then
            echo -e "${RED}Error:${RESET} BEAM source directory not found: $BEAM_SRC"
            ok=false
        fi
        if ! command -v erlc &>/dev/null; then
            echo -e "${RED}Error:${RESET} erlc not found (needed with --beam-src)"
            ok=false
        fi
    fi

    if [ -n "$RUST_SRC" ]; then
        if [ ! -d "$RUST_SRC" ]; then
            echo -e "${RED}Error:${RESET} Rust source directory not found: $RUST_SRC"
            ok=false
        fi
        if ! command -v cargo &>/dev/null; then
            echo -e "${RED}Error:${RESET} cargo not found (needed with --rust-src)"
            ok=false
        fi
    fi

    $ok || exit 1
}

# ─── Measurement helpers ──────────────────────────────────────────────

get_rss_kb() {
    local pid=$1
    ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo "0"
}

get_footprint_mb() {
    local pid=$1
    if [[ "$(uname)" != "Darwin" ]]; then
        echo "N/A"
        return
    fi
    if command -v footprint &>/dev/null; then
        local tmpjson="/tmp/.onecached_fp_$$.json"
        footprint --pid "$pid" --json "$tmpjson" >/dev/null 2>&1 || true
        local result
        result=$(python3 -c "
import json
try:
    with open('$tmpjson') as f:
        data = json.load(f)
    procs = data.get('processes', [])
    if len(procs) > 0:
        fp = procs[0].get('footprint', 0)
        print(f'{fp / 1048576:.2f}')
    else:
        print('N/A')
except:
    print('N/A')
" 2>/dev/null || echo "N/A")
        rm -f "$tmpjson"
        echo "$result"
    else
        echo "N/A"
    fi
}

nc_send() {
    # macOS nc doesn't support -q; use -w for timeout
    local data=$1
    local port=$2
    local outfile=${3:-/dev/null}
    printf '%s\r\n' "$data" | nc -w 1 127.0.0.1 "$port" > "$outfile" 2>/dev/null || true
}

get_memcached_stats() {
    local outfile=$1
    local port=$2
    nc_send "stats" "$port" "$outfile"
    [ -s "$outfile" ] || echo "(stats capture failed)" > "$outfile"
}

send_flush() {
    local port=$1
    nc_send "flush_all" "$port"
}

capture_stage() {
    local pid=$1
    local stage=$2
    local run_dir=$3
    local port=$4

    local rss_kb
    rss_kb=$(get_rss_kb "$pid")
    local fp_mb
    fp_mb=$(get_footprint_mb "$pid")
    echo "${stage},${rss_kb}" >> "$run_dir/rss.csv"
    echo "${stage},${fp_mb}" >> "$run_dir/footprint.csv"
    get_memcached_stats "$run_dir/stats_${stage}.txt" "$port"
}

wait_for_port() {
    local port=$1
    local timeout=${2:-10}
    local elapsed=0
    while ! nc -z 127.0.0.1 "$port" 2>/dev/null; do
        sleep 0.2
        elapsed=$((elapsed + 1))
        if [ "$elapsed" -ge "$((timeout * 5))" ]; then
            echo -e "${RED}Timeout waiting for port $port${RESET}"
            return 1
        fi
    done
}

kill_port() {
    # Kill any process listening on the benchmark port
    local port=$1
    local pids
    pids=$(lsof -ti :"$port" 2>/dev/null || true)
    if [ -n "$pids" ]; then
        echo "$pids" | xargs kill -9 2>/dev/null || true
        sleep 1
    fi
}

stop_server() {
    local pid=$1
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        # Wait up to 5s for graceful shutdown
        for _ in $(seq 25); do
            kill -0 "$pid" 2>/dev/null || return 0
            sleep 0.2
        done
        # Force kill
        kill -9 "$pid" 2>/dev/null || true
        sleep 0.5
    fi
}

# ─── Server start functions ──────────────────────────────────────────

start_beam() {
    local logfile=$1
    # Keep ETS ownership in a dedicated process so evaluator crashes do not
    # delete tables while the application is still serving requests.
    erl -noshell -pa "$BEAM_DIR" \
        -eval '
Parent = self(),
Owner = spawn(fun() ->
    onecached_storage:init(ets),
    Parent ! onecached_storage_ready,
    receive stop -> ok end
end),
receive
    onecached_storage_ready -> ok
after 5000 ->
    exit(storage_init_timeout)
end,
application:start(sasl),
application:start(onecached),
receive
    stop ->
        Owner ! stop,
        ok
end.' \
        >"$logfile" 2>&1 &
    echo $!
}

start_nera() {
    local logfile=$1
    "$NERA_BINARY" >"$logfile" 2>&1 &
    echo $!
}

start_rust() {
    local logfile=$1
    local port=$2
    # The in-repo `benchmarks/rust_baseline` binary takes the port as
    # positional arg 1. Older external baselines may read it from the
    # $PORT env var instead — pass both so either convention works.
    PORT="$port" "$RUST_BINARY" "$port" >"$logfile" 2>&1 &
    echo $!
}

# ─── Extract metrics from memtier JSON ────────────────────────────────

extract_from_json() {
    local json_file=$1
    # Extract key metrics from memtier JSON output
    python3 -c "
import json, sys
try:
    with open('$json_file') as f:
        data = json.load(f)
    totals = data['ALL STATS']['Totals']
    print(f\"ops_sec={totals['Ops/sec']:.2f}\")
    print(f\"avg_latency={totals['Latency']:.5f}\")
    print(f\"p50_latency={totals['Percentile Latencies']['p50.00']:.3f}\")
    print(f\"p99_latency={totals['Percentile Latencies']['p99.00']:.3f}\")
    print(f\"p999_latency={totals['Percentile Latencies']['p99.90']:.3f}\")
    print(f\"kb_sec={totals['KB/sec']:.2f}\")
except Exception as e:
    print(f'parse_error={e}', file=sys.stderr)
    # Fallback: try to extract from text
    print('ops_sec=0')
    print('avg_latency=0')
    print('p50_latency=0')
    print('p99_latency=0')
    print('p999_latency=0')
    print('kb_sec=0')
" 2>/dev/null
}

# ─── Timeout-aware memtier runner ─────────────────────────────────────

run_memtier() {
    # Run memtier_benchmark with a timeout watchdog.
    # Args: all arguments are passed to memtier_benchmark
    local timeout_sec=$MEMTIER_TIMEOUT
    memtier_benchmark "$@" &
    local mt_pid=$!
    local elapsed=0
    while kill -0 "$mt_pid" 2>/dev/null; do
        sleep 1
        elapsed=$((elapsed + 1))
        if [ "$elapsed" -ge "$timeout_sec" ]; then
            kill -9 "$mt_pid" 2>/dev/null || true
            wait "$mt_pid" 2>/dev/null || true
            return 1
        fi
    done
    wait "$mt_pid" 2>/dev/null
    return $?
}

# ─── BEAM compile from source ────────────────────────────────────────

compile_beam_src() {
    if [ -z "$BEAM_SRC" ]; then
        return
    fi
    echo -e "${DIM}Compiling BEAM files from $BEAM_SRC ...${RESET}"
    mkdir -p "$BEAM_DIR"
    local compile_log="$BEAM_DIR/compile.log"
    : > "$compile_log"
    local ok=true
    local beam_files=("$BEAM_SRC"/*.erl)
    if [ ! -e "${beam_files[0]}" ]; then
        echo -e "${RED}Error:${RESET} no .erl files found in $BEAM_SRC"
        exit 1
    fi
    # OneCached includes live in a sibling `include/` dir alongside `src/`.
    local onecached_include="$BEAM_SRC/../include"
    for f in "${beam_files[@]}"; do
        if ! erlc -o "$BEAM_DIR" -I "$BEAM_SRC" -I "$onecached_include" "$f" >>"$compile_log" 2>&1; then
            echo -e "${RED}Error:${RESET} failed compiling $(basename "$f")"
            ok=false
        fi
    done
    if [ -f "$BEAM_SRC/onecached.app" ]; then
        cp "$BEAM_SRC/onecached.app" "$BEAM_DIR/onecached.app"
    else
        echo -e "${YELLOW}Warning:${RESET} onecached.app not found in $BEAM_SRC"
    fi
    if [ "$ok" = false ]; then
        echo -e "${RED}Error:${RESET} BEAM compile failed (see $compile_log)"
        exit 1
    fi
}

# ─── Rust baseline build from source ─────────────────────────────────

compile_rust_src() {
    if [ -z "$RUST_SRC" ]; then
        return
    fi
    if [ ! -f "$RUST_SRC/Cargo.toml" ]; then
        echo -e "${RED}Error:${RESET} --rust-src '$RUST_SRC' has no Cargo.toml"
        exit 1
    fi
    echo -e "${DIM}Building Rust baseline from $RUST_SRC ...${RESET}"
    local compile_log="$RUST_SRC/target/compile.log"
    mkdir -p "$(dirname "$compile_log")"
    if ! ( cd "$RUST_SRC" && cargo build --release --quiet ) >"$compile_log" 2>&1; then
        echo -e "${RED}Error:${RESET} Rust baseline build failed (see $compile_log)"
        exit 1
    fi
    local built_bin="$RUST_SRC/target/release/rust_mcached_baseline"
    if [ ! -x "$built_bin" ]; then
        echo -e "${RED}Error:${RESET} expected binary not found: $built_bin"
        exit 1
    fi
    RUST_BINARY="$built_bin"
}

# ─── Single run ──────────────────────────────────────────────────────

run_single() {
    local engine=$1
    local run_num=$2
    local run_dir="$OUTPUT_DIR/$engine/run_${run_num}"
    local engine_port
    engine_port=$(port_for_engine "$engine")
    mkdir -p "$run_dir"

    # Clean port
    kill_port "$engine_port"

    # Start server
    local pid
    case "$engine" in
        beam) pid=$(start_beam "$run_dir/server.log") ;;
        nera) pid=$(start_nera "$run_dir/server.log") ;;
        rust) pid=$(start_rust "$run_dir/server.log" "$engine_port") ;;
    esac
    echo "$pid" > "$run_dir/server.pid"

    # Wait for ready
    if ! wait_for_port "$engine_port" 10; then
        echo -e "  ${RED}Server failed to start${RESET}"
        stop_server "$pid"
        return 1
    fi

    # Check server is still alive after port comes up
    sleep 0.5
    if ! kill -0 "$pid" 2>/dev/null; then
        echo -e "  ${RED}Server died shortly after start${RESET}"
        return 1
    fi

    # Init CSV headers
    echo "stage,rss_kb" > "$run_dir/rss.csv"
    echo "stage,footprint_mb" > "$run_dir/footprint.csv"

    # Stage: start
    capture_stage "$pid" "start" "$run_dir" "$engine_port"

    # Warmup (discard)
    run_memtier --protocol=memcache_text \
        --server=127.0.0.1 --port="$engine_port" \
        --clients="$MEMTIER_CLIENTS" --threads="$MEMTIER_THREADS" \
        --ratio="$RATIO" --data-size="$DATA_SIZE" \
        --pipeline=1 --test-time="$WARMUP_TIME" \
        >/dev/null 2>&1 || true

    # Check server survived warmup
    if ! kill -0 "$pid" 2>/dev/null; then
        echo -e "  ${RED}Server died during warmup${RESET}"
        return 1
    fi

    # Stage: post-warmup
    capture_stage "$pid" "post_warmup" "$run_dir" "$engine_port"

    # Measured run
    run_memtier --protocol=memcache_text \
        --server=127.0.0.1 --port="$engine_port" \
        --clients="$MEMTIER_CLIENTS" --threads="$MEMTIER_THREADS" \
        --ratio="$RATIO" --data-size="$DATA_SIZE" \
        --pipeline=1 --test-time="$TEST_TIME" \
        --json-out-file="$run_dir/memtier.json" \
        > "$run_dir/memtier.txt" 2>&1 || true

    # Stage: after_load
    capture_stage "$pid" "after_load" "$run_dir" "$engine_port"

    # Idle 10s
    sleep 10
    capture_stage "$pid" "after_idle" "$run_dir" "$engine_port"

    # Flush
    send_flush "$engine_port"

    sleep 1
    capture_stage "$pid" "after_flush1s" "$run_dir" "$engine_port"

    sleep 9
    capture_stage "$pid" "after_flush10s" "$run_dir" "$engine_port"

    if [ "$SKIP_FLUSH_WAIT" = false ]; then
        sleep 50
        capture_stage "$pid" "after_flush60s" "$run_dir" "$engine_port"
    fi

    # Stop server
    stop_server "$pid"

    # Extract metrics
    if [ -f "$run_dir/memtier.json" ]; then
        extract_from_json "$run_dir/memtier.json" > "$run_dir/metrics.txt"
    fi

    return 0
}

# ─── Summary computation ──────────────────────────────────────────────

compute_engine_summary() {
    local engine=$1
    local engine_dir="$OUTPUT_DIR/$engine"

    python3 -c "
import os, json, csv, sys

engine_dir = '$engine_dir'
runs = sorted([d for d in os.listdir(engine_dir) if d.startswith('run_')])

ops_list = []
avg_lat_list = []
p50_list = []
p99_list = []
p999_list = []
rss_load_list = []
rss_flush_list = []
fp_load_list = []
fp_flush_list = []

for run in runs:
    run_dir = os.path.join(engine_dir, run)

    # Memtier metrics
    metrics_file = os.path.join(run_dir, 'metrics.txt')
    if os.path.exists(metrics_file):
        metrics = {}
        for line in open(metrics_file):
            if '=' in line:
                k, v = line.strip().split('=', 1)
                try:
                    metrics[k] = float(v)
                except:
                    pass
        if 'ops_sec' in metrics and metrics['ops_sec'] > 0:
            ops_list.append(metrics['ops_sec'])
            avg_lat_list.append(metrics.get('avg_latency', 0))
            p50_list.append(metrics.get('p50_latency', 0))
            p99_list.append(metrics.get('p99_latency', 0))
            p999_list.append(metrics.get('p999_latency', 0))

    # RSS
    rss_file = os.path.join(run_dir, 'rss.csv')
    if os.path.exists(rss_file):
        rss = {}
        for line in open(rss_file):
            parts = line.strip().split(',')
            if len(parts) == 2 and parts[0] != 'stage':
                try:
                    rss[parts[0]] = int(parts[1])
                except:
                    pass
        if 'after_load' in rss:
            rss_load_list.append(rss['after_load'] / 1024)  # KB → MB
        flush_key = 'after_flush60s' if 'after_flush60s' in rss else 'after_flush10s'
        if flush_key in rss:
            rss_flush_list.append(rss[flush_key] / 1024)

    # Footprint
    fp_file = os.path.join(run_dir, 'footprint.csv')
    if os.path.exists(fp_file):
        fp = {}
        for line in open(fp_file):
            parts = line.strip().split(',')
            if len(parts) == 2 and parts[0] != 'stage':
                try:
                    fp[parts[0]] = float(parts[1])
                except:
                    pass
        if 'after_load' in fp:
            fp_load_list.append(fp['after_load'])
        flush_key = 'after_flush60s' if 'after_flush60s' in fp else 'after_flush10s'
        if flush_key in fp:
            fp_flush_list.append(fp[flush_key])

def median(lst):
    if not lst:
        return 0
    s = sorted(lst)
    n = len(s)
    if n % 2 == 1:
        return s[n // 2]
    return (s[n // 2 - 1] + s[n // 2]) / 2

# Write summary CSV
with open(os.path.join(engine_dir, 'summary.csv'), 'w') as f:
    writer = csv.writer(f)
    writer.writerow(['run', 'ops_sec', 'avg_lat', 'p50', 'p99', 'p999', 'rss_load_mb', 'rss_flush_mb', 'fp_load_mb', 'fp_flush_mb'])
    for i in range(len(ops_list)):
        writer.writerow([
            i + 1,
            f'{ops_list[i]:.0f}' if i < len(ops_list) else '',
            f'{avg_lat_list[i]:.3f}' if i < len(avg_lat_list) else '',
            f'{p50_list[i]:.3f}' if i < len(p50_list) else '',
            f'{p99_list[i]:.3f}' if i < len(p99_list) else '',
            f'{p999_list[i]:.3f}' if i < len(p999_list) else '',
            f'{rss_load_list[i]:.1f}' if i < len(rss_load_list) else '',
            f'{rss_flush_list[i]:.1f}' if i < len(rss_flush_list) else '',
            f'{fp_load_list[i]:.2f}' if i < len(fp_load_list) else '',
            f'{fp_flush_list[i]:.2f}' if i < len(fp_flush_list) else '',
        ])

# Print medians as key=value for shell consumption
print(f'ops_sec={median(ops_list):.0f}')
print(f'avg_latency={median(avg_lat_list):.3f}')
print(f'p50_latency={median(p50_list):.3f}')
print(f'p99_latency={median(p99_list):.3f}')
print(f'p999_latency={median(p999_list):.3f}')
print(f'rss_load_mb={median(rss_load_list):.1f}')
print(f'rss_flush_mb={median(rss_flush_list):.1f}')
print(f'fp_load_mb={median(fp_load_list):.2f}')
print(f'fp_flush_mb={median(fp_flush_list):.2f}')
print(f'num_runs={len(ops_list)}')

# Variation check
if len(ops_list) >= 3:
    mn, mx = min(ops_list), max(ops_list)
    mid = median(ops_list)
    variation = (mx - mn) / mid * 100 if mid > 0 else 0
    print(f'variation_pct={variation:.1f}')
"
}

# ─── Final comparison table ──────────────────────────────────────────

print_comparison() {
    local comp_file="$OUTPUT_DIR/comparison.txt"

    # Collect medians for each engine
    declare -A MEDIANS

    IFS=',' read -ra ENGINE_LIST <<< "$ENGINES"
    for eng in "${ENGINE_LIST[@]}"; do
        local summary_file="$OUTPUT_DIR/$eng/summary_medians.txt"
        if [ -f "$summary_file" ]; then
            while IFS='=' read -r key val; do
                MEDIANS["${eng}_${key}"]="$val"
            done < "$summary_file"
        fi
    done

    # Print table
    {
        echo ""
        echo "OneCached Benchmark — $(date '+%Y-%m-%d %H:%M')"
        echo "nera git: $(nera_git_sha)"
        for eng in "${ENGINE_LIST[@]}"; do
            local _bp=""
            case "$eng" in
                nera) _bp="$NERA_BINARY" ;;
                rust) _bp="$RUST_BINARY" ;;
                *) continue ;;
            esac
            [ -f "$_bp" ] && echo "$eng binary: $_bp  sha256=$(binary_sha256_short "$_bp")  mtime=$(binary_mtime "$_bp")"
        done
        echo "memtier: --clients=$MEMTIER_CLIENTS --threads=$MEMTIER_THREADS --ratio=$RATIO --data-size=$DATA_SIZE --test-time=$TEST_TIME"
        echo "Runs per engine: $RUNS"
        echo ""
        printf "%-8s  %10s  %10s  %10s  %10s  %10s  %10s  %10s  %10s\n" \
            "Engine" "Ops/sec" "Avg(ms)" "p50(ms)" "p99(ms)" "p99.9(ms)" "RSS load" "RSS flush" "Footprint"
        printf "%-8s  %10s  %10s  %10s  %10s  %10s  %10s  %10s  %10s\n" \
            "------" "-------" "-------" "-------" "-------" "---------" "--------" "---------" "---------"

        for eng in "${ENGINE_LIST[@]}"; do
            local ops="${MEDIANS[${eng}_ops_sec]:-N/A}"
            local avg="${MEDIANS[${eng}_avg_latency]:-N/A}"
            local p50="${MEDIANS[${eng}_p50_latency]:-N/A}"
            local p99="${MEDIANS[${eng}_p99_latency]:-N/A}"
            local p999="${MEDIANS[${eng}_p999_latency]:-N/A}"
            local rss_l="${MEDIANS[${eng}_rss_load_mb]:-N/A}"
            local rss_f="${MEDIANS[${eng}_rss_flush_mb]:-N/A}"
            local fp_f="${MEDIANS[${eng}_fp_flush_mb]:-N/A}"

            # Format RSS with MB suffix
            [[ "$rss_l" != "N/A" ]] && rss_l="${rss_l} MB"
            [[ "$rss_f" != "N/A" ]] && rss_f="${rss_f} MB"
            [[ "$fp_f" != "N/A" ]] && fp_f="${fp_f} MB"

            local label
            case "$eng" in
                beam) label="BEAM" ;;
                nera) label="Nera" ;;
                rust) label="Rust" ;;
                *) label="$eng" ;;
            esac

            printf "%-8s  %10s  %10s  %10s  %10s  %10s  %10s  %10s  %10s\n" \
                "$label" "$ops" "$avg" "$p50" "$p99" "$p999" "$rss_l" "$rss_f" "$fp_f"
        done
        echo ""

        # Variation warnings
        for eng in "${ENGINE_LIST[@]}"; do
            local var="${MEDIANS[${eng}_variation_pct]:-}"
            if [ -n "$var" ]; then
                local var_int=${var%%.*}
                if [ "$var_int" -gt 10 ] 2>/dev/null; then
                    echo "⚠ $eng: ops/sec variation ${var}% exceeds 10% target"
                fi
            fi
        done

        echo "(all values are medians across $RUNS runs)"
    } | tee "$comp_file"
}

# ─── Provenance ──────────────────────────────────────────────────────

nera_git_sha() {
    local sha dirty=""
    sha=$(git -C "$SCRIPT_DIR/.." rev-parse --short=12 HEAD 2>/dev/null) || { echo "unknown"; return; }
    git -C "$SCRIPT_DIR/.." diff --quiet HEAD 2>/dev/null || dirty="-dirty"
    echo "${sha}${dirty}"
}

binary_sha256_short() {
    # Print first 16 hex chars of SHA-256 for a binary file
    local path=$1
    if [ ! -f "$path" ]; then echo "missing"; return; fi
    if command -v shasum &>/dev/null; then
        shasum -a 256 "$path" | cut -c1-16
    elif command -v sha256sum &>/dev/null; then
        sha256sum "$path" | cut -c1-16
    else
        echo "no-sha256-tool"
    fi
}

binary_mtime() {
    local path=$1
    if [ ! -f "$path" ]; then echo "missing"; return; fi
    if [[ "$(uname)" == "Darwin" ]]; then
        stat -f '%Sm' -t '%Y-%m-%d %H:%M:%S' "$path"
    else
        stat -c '%y' "$path" 2>/dev/null | cut -d. -f1
    fi
}

print_provenance() {
    local git_sha
    git_sha=$(nera_git_sha)
    echo -e "${DIM}nera git: ${git_sha}${RESET}"

    IFS=',' read -ra ENGINE_LIST <<< "$ENGINES"
    for eng in "${ENGINE_LIST[@]}"; do
        local bin_path="" label=""
        case "$eng" in
            nera) bin_path="$NERA_BINARY"; label="Nera binary" ;;
            rust) bin_path="$RUST_BINARY"; label="Rust binary" ;;
            beam) continue ;;
            *)    continue ;;
        esac
        if [ -f "$bin_path" ]; then
            local sha mtime
            sha=$(binary_sha256_short "$bin_path")
            mtime=$(binary_mtime "$bin_path")
            echo -e "${DIM}${label}: ${bin_path}${RESET}"
            echo -e "${DIM}  sha256=${sha}  mtime=${mtime}${RESET}"
        else
            echo -e "${YELLOW}${label}: ${bin_path} (NOT FOUND)${RESET}"
        fi
    done
}

# ─── Environment capture ──────────────────────────────────────────────

capture_env() {
    python3 -c "
import json, platform, subprocess, os

env = {
    'timestamp': '$(date -u +%Y-%m-%dT%H:%M:%SZ)',
    'os': platform.system(),
    'os_version': platform.version(),
    'machine': platform.machine(),
    'cpu': platform.processor() or 'unknown',
}

# CPU count
try:
    env['cpu_count'] = os.cpu_count()
except:
    pass

# Total memory
try:
    if platform.system() == 'Darwin':
        out = subprocess.check_output(['sysctl', '-n', 'hw.memsize'], text=True).strip()
        env['memory_gb'] = round(int(out) / (1024**3), 1)
    else:
        with open('/proc/meminfo') as f:
            for line in f:
                if 'MemTotal' in line:
                    kb = int(line.split()[1])
                    env['memory_gb'] = round(kb / (1024**2), 1)
                    break
except:
    pass

# Tool versions
try:
    out = subprocess.check_output(['memtier_benchmark', '--version'], text=True, stderr=subprocess.STDOUT)
    env['memtier_version'] = out.strip().split('\\n')[0]
except:
    pass

try:
    out = subprocess.check_output(['erl', '-noshell', '-eval', 'io:format(\"~s\", [erlang:system_info(otp_release)]), halt().'], text=True, stderr=subprocess.STDOUT)
    env['otp_release'] = out.strip()
except:
    pass

# Git SHA of nera repo (compiler/runtime provenance)
try:
    sha = subprocess.check_output(
        ['git', 'rev-parse', '--short=12', 'HEAD'],
        text=True, stderr=subprocess.DEVNULL,
        cwd='$SCRIPT_DIR/..'
    ).strip()
    dirty = subprocess.call(
        ['git', 'diff', '--quiet', 'HEAD'],
        stderr=subprocess.DEVNULL,
        cwd='$SCRIPT_DIR/..'
    )
    env['nera_git_sha'] = sha + ('-dirty' if dirty else '')
except:
    env['nera_git_sha'] = 'unknown'

# Binary provenance: path + SHA-256 + mtime for each engine
import hashlib
def binary_info(path):
    info = {'path': path}
    try:
        with open(path, 'rb') as bf:
            info['sha256'] = hashlib.sha256(bf.read()).hexdigest()[:16]
        info['mtime'] = os.path.getmtime(path)
        from datetime import datetime
        info['mtime_human'] = datetime.fromtimestamp(info['mtime']).strftime('%Y-%m-%d %H:%M:%S')
    except:
        info['sha256'] = 'missing'
    return info

binaries = {}
nera_bin = '$NERA_BINARY'
if os.path.exists(nera_bin):
    binaries['nera'] = binary_info(nera_bin)
rust_bin = '$RUST_BINARY'
if os.path.exists(rust_bin):
    binaries['rust'] = binary_info(rust_bin)
if binaries:
    env['binaries'] = binaries

print(json.dumps(env, indent=2))
" > "$OUTPUT_DIR/env.json"
}

# ─── Main ─────────────────────────────────────────────────────────────

main() {
    resolve_engine_ports
    check_prereqs

    mkdir -p "$OUTPUT_DIR"

    echo ""
    echo -e "${BOLD}OneCached Benchmark Harness — Phase 0${RESET}"
    echo -e "${DIM}Output: $OUTPUT_DIR${RESET}"
    echo -e "${DIM}Engines: $ENGINES | Runs: $RUNS | Test: ${TEST_TIME}s | Data: ${DATA_SIZE}B${RESET}"
    echo -e "${DIM}Ports: beam=$BEAM_PORT nera=$NERA_PORT rust=$RUST_PORT${RESET}"
    echo ""

    # Binary & git provenance (catch stale binaries early)
    print_provenance
    echo ""

    # Compile BEAM / Rust baseline from source if requested
    compile_beam_src
    compile_rust_src

    # Capture environment
    capture_env

    IFS=',' read -ra ENGINE_LIST <<< "$ENGINES"

    for engine in "${ENGINE_LIST[@]}"; do
        local label
        case "$engine" in
            beam) label="BEAM" ;;
            nera) label="Nera" ;;
            rust) label="Rust" ;;
            *) label="$engine" ;;
        esac

        echo -e "${CYAN}═══ $label ═══${RESET}"
        mkdir -p "$OUTPUT_DIR/$engine"

        for run in $(seq 1 "$RUNS"); do
            echo -e -n "  Run $run/$RUNS ... "

            if run_single "$engine" "$run"; then
                # Quick stats from this run
                if [ -f "$OUTPUT_DIR/$engine/run_${run}/metrics.txt" ]; then
                    local ops
                    ops=$(grep 'ops_sec=' "$OUTPUT_DIR/$engine/run_${run}/metrics.txt" | cut -d= -f2)
                    local rss
                    rss=$(tail -1 "$OUTPUT_DIR/$engine/run_${run}/rss.csv" | cut -d, -f2)
                    local rss_mb=""
                    if [ -n "$rss" ] && [ "$rss" != "rss_kb" ]; then
                        rss_mb=$(echo "scale=1; $rss / 1024" | bc 2>/dev/null || echo "?")
                    fi
                    echo -e "${GREEN}${ops:-?} ops/sec${RESET}  RSS: ${rss_mb:-?} MB"
                else
                    echo -e "${YELLOW}(no metrics)${RESET}"
                fi
            else
                echo -e "${RED}FAILED${RESET}"
            fi
        done

        # Compute summary for this engine
        compute_engine_summary "$engine" > "$OUTPUT_DIR/$engine/summary_medians.txt"

        echo ""
    done

    # Final comparison
    echo -e "${BOLD}═══ Comparison (medians) ═══${RESET}"
    print_comparison

    echo ""
    echo -e "${DIM}Full results: $OUTPUT_DIR${RESET}"
    echo -e "${BOLD}Done.${RESET}"
}

main
