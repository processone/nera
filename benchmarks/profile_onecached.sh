#!/usr/bin/env bash
#
# OneCached Flamegraph Profiling Script
#
# Profiles nera-compiled OneCached under load using samply.
# Opens the Firefox Profiler UI automatically with the captured profile.
#
# Usage:
#   ./benchmarks/profile_onecached.sh [OPTIONS]
#
# Options:
#   --test-time T       memtier load duration in seconds (default: 15)
#   --data-size S       Value size in bytes (default: 256)
#   --clients N         memtier clients (default: 50)
#   --threads N         memtier threads (default: 4)
#   --ratio R           set:get ratio (default: 1:1)
#   --port PORT         OneCached port (default: 11211)
#   --nera-binary PATH  Use pre-built nera binary instead of building
#   --onecached-src DIR OneCached source (default: ~/devel/onecached/src)
#   --skip-build        Skip building nera (use existing binary)
#   --warmup-time T     Warmup duration in seconds (default: 3)
#   --save FILE         Save profile to file (samply .json.gz format)
#
# Prerequisites:
#   - samply (cargo install samply)
#   - memtier_benchmark (brew install memtier_benchmark)
#
set -euo pipefail

# ─── Defaults ──────────────────────────────────────────────────────────
TEST_TIME=15
DATA_SIZE=256
CLIENTS=50
THREADS=4
RATIO="1:1"
PORT=11211
NERA_BINARY=""
ONECACHED_SRC="$HOME/devel/onecached/src"
SKIP_BUILD=false
WARMUP_TIME=3
SAVE_FILE=""
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WORK_DIR="/tmp/onecached-profile"

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
        --test-time)      TEST_TIME="$2"; shift 2 ;;
        --data-size)      DATA_SIZE="$2"; shift 2 ;;
        --clients)        CLIENTS="$2"; shift 2 ;;
        --threads)        THREADS="$2"; shift 2 ;;
        --ratio)          RATIO="$2"; shift 2 ;;
        --port)           PORT="$2"; shift 2 ;;
        --nera-binary)    NERA_BINARY="$2"; shift 2 ;;
        --onecached-src)  ONECACHED_SRC="$2"; shift 2 ;;
        --skip-build)     SKIP_BUILD=true; shift ;;
        --warmup-time)    WARMUP_TIME="$2"; shift 2 ;;
        --save)           SAVE_FILE="$2"; shift 2 ;;
        --help|-h)
            sed -n '2,/^set /p' "$0" | grep '^#' | sed 's/^# \?//'
            exit 0 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# ─── Prerequisites ─────────────────────────────────────────────────────
check_prereqs() {
    local ok=true
    if ! command -v samply &>/dev/null; then
        echo -e "${RED}Error:${RESET} samply not found. Install with: cargo install samply"
        ok=false
    fi
    if ! command -v memtier_benchmark &>/dev/null; then
        echo -e "${RED}Error:${RESET} memtier_benchmark not found. Install with: brew install memtier_benchmark"
        ok=false
    fi
    if [ ! -d "$ONECACHED_SRC" ]; then
        echo -e "${RED}Error:${RESET} OneCached source not found: $ONECACHED_SRC"
        ok=false
    fi
    $ok || exit 1
}

# ─── Build nera with debug symbols ─────────────────────────────────────
build_nera() {
    if $SKIP_BUILD && [ -n "$NERA_BINARY" ] && [ -x "$NERA_BINARY" ]; then
        echo -e "${DIM}Skipping build, using: $NERA_BINARY${RESET}"
        return
    fi

    echo -e "${CYAN}Building nera (release + debug symbols)...${RESET}"

    # Build with debug info for profiling
    CARGO_PROFILE_RELEASE_DEBUG=2 cargo build --release \
        --manifest-path "$PROJECT_DIR/Cargo.toml" 2>&1 | tail -3

    NERA_BINARY="$PROJECT_DIR/target/release/nera"
    echo -e "${GREEN}Built: $NERA_BINARY${RESET}"
}

# ─── Compile OneCached with nera ────────────────────────────────────────
compile_onecached() {
    mkdir -p "$WORK_DIR"
    local erl_files
    erl_files=$(find "$ONECACHED_SRC" -name '*.erl' -type f | sort | tr '\n' ' ')

    echo -e "${CYAN}Compiling OneCached with nera...${RESET}"
    # OneCached includes live in a sibling `include/` dir alongside `src/`.
    local onecached_root
    onecached_root="$(cd "$ONECACHED_SRC/.." && pwd)"
    local include_dir="$onecached_root/include"
    # Run from WORK_DIR so nera's auto-tests-discovery (which walks CWD/tests)
    # doesn't pick up unrelated .erl files from the nera repo's tests/ tree.
    ( cd "$WORK_DIR" && "$NERA_BINARY" build $erl_files \
        -I "$ONECACHED_SRC" \
        -I "$include_dir" \
        -o "$WORK_DIR/onecached_nera" \
        -e onecached:start )

    chmod +x "$WORK_DIR/onecached_nera"
    echo -e "${GREEN}Compiled: $WORK_DIR/onecached_nera${RESET}"
}

# ─── Port management ───────────────────────────────────────────────────
kill_port() {
    local p=$1
    local pids
    pids=$(lsof -ti :"$p" 2>/dev/null || true)
    if [ -n "$pids" ]; then
        echo -e "${YELLOW}Killing processes on port $p${RESET}"
        echo "$pids" | xargs kill -9 2>/dev/null || true
        sleep 1
    fi
}

wait_for_port() {
    local p=$1 timeout=$2
    for i in $(seq 1 "$timeout"); do
        if nc -z 127.0.0.1 "$p" 2>/dev/null; then
            return 0
        fi
        sleep 1
    done
    return 1
}

# ─── Cleanup ────────────────────────────────────────────────────────────
SAMPLY_PID=""
cleanup() {
    if [ -n "${MEMTIER_PID:-}" ]; then
        kill "$MEMTIER_PID" 2>/dev/null || true
    fi
    # Kill the child (onecached) so samply saves the profile, then wait
    if [ -n "$SAMPLY_PID" ]; then
        local child
        child=$(pgrep -P "$SAMPLY_PID" onecached_nera 2>/dev/null || true)
        [ -n "$child" ] && kill "$child" 2>/dev/null || true
        wait "$SAMPLY_PID" 2>/dev/null || true
    fi
    # Fallback: clean port if still in use
    kill_port "$PORT"
}
trap cleanup EXIT

# ─── Main ───────────────────────────────────────────────────────────────
main() {
    check_prereqs
    build_nera
    compile_onecached

    # Clean port
    kill_port "$PORT"

    local profile_file="$WORK_DIR/profile.json.gz"

    echo ""
    echo -e "${BOLD}=== Profiling OneCached on port $PORT ===${RESET}"
    echo -e "${DIM}Load: $CLIENTS clients x $THREADS threads, ratio=$RATIO, data=$DATA_SIZE bytes, ${TEST_TIME}s${RESET}"
    echo ""

    # Launch OneCached under samply (samply runs it as a child process).
    # Use --save-only so samply doesn't try to open a browser while recording.
    echo -e "${CYAN}Starting OneCached under samply profiler...${RESET}"
    samply record \
        --rate 997 \
        --save-only \
        --output "$profile_file" \
        "$WORK_DIR/onecached_nera" \
        >"$WORK_DIR/samply.log" 2>&1 &
    SAMPLY_PID=$!

    # Wait for server to be ready
    if ! wait_for_port "$PORT" 15; then
        echo -e "${RED}OneCached failed to start on port $PORT${RESET}"
        echo -e "${RED}samply log:${RESET}"
        cat "$WORK_DIR/samply.log" 2>/dev/null || true
        exit 1
    fi
    echo -e "${GREEN}OneCached ready on port $PORT (samply PID $SAMPLY_PID)${RESET}"

    # Warmup
    echo -e "${CYAN}Warmup (${WARMUP_TIME}s)...${RESET}"
    memtier_benchmark \
        --protocol=memcache_text \
        --server=127.0.0.1 --port="$PORT" \
        --clients="$CLIENTS" --threads="$THREADS" \
        --ratio="$RATIO" --data-size="$DATA_SIZE" \
        --test-time="$WARMUP_TIME" --pipeline=1 \
        --hide-histogram \
        >"$WORK_DIR/warmup.txt" 2>&1
    echo -e "${GREEN}Warmup done${RESET}"

    # Measured run under profile
    echo -e "${BOLD}${CYAN}Running load (${TEST_TIME}s) — profiling active...${RESET}"
    memtier_benchmark \
        --protocol=memcache_text \
        --server=127.0.0.1 --port="$PORT" \
        --clients="$CLIENTS" --threads="$THREADS" \
        --ratio="$RATIO" --data-size="$DATA_SIZE" \
        --test-time="$TEST_TIME" --pipeline=1 \
        --json-out-file="$WORK_DIR/memtier.json" \
        2>&1 | tee "$WORK_DIR/memtier.txt"

    echo ""
    echo -e "${CYAN}Load complete. Stopping server...${RESET}"

    # Kill the child process (onecached_nera) to trigger samply's profile save.
    # samply only saves when its child exits — sending signals to samply itself doesn't work.
    local child_pid
    child_pid=$(pgrep -P "$SAMPLY_PID" onecached_nera 2>/dev/null || true)
    if [ -n "$child_pid" ]; then
        echo -e "${DIM}Stopping OneCached (PID $child_pid)...${RESET}"
        kill "$child_pid" 2>/dev/null || true
    else
        # Fallback: kill via port
        kill_port "$PORT"
    fi

    # Wait for samply to finish saving the profile
    echo -e "${DIM}Waiting for samply to save profile...${RESET}"
    wait "$SAMPLY_PID" 2>/dev/null || true
    SAMPLY_PID=""

    # Verify profile was saved
    if [ ! -f "$profile_file" ]; then
        echo -e "${RED}Profile file not created. samply log:${RESET}"
        cat "$WORK_DIR/samply.log" 2>/dev/null || true
        exit 1
    fi

    local profile_size
    profile_size=$(du -h "$profile_file" | cut -f1)
    echo ""
    echo -e "${GREEN}${BOLD}Profiling complete!${RESET} (${profile_size})"

    if [ -n "$SAVE_FILE" ]; then
        cp "$profile_file" "$SAVE_FILE"
        echo -e "Profile saved to: ${BOLD}$SAVE_FILE${RESET}"
    fi

    # Open the profile in Firefox Profiler
    echo -e "Profile: ${BOLD}$profile_file${RESET}"
    echo -e "${CYAN}Opening in Firefox Profiler...${RESET}"
    samply load --no-open "$profile_file" &
    local LOAD_PID=$!
    sleep 2

    # Find the port samply is serving on and open in Firefox
    local samply_port
    samply_port=$(lsof -iTCP -sTCP:LISTEN -p "$LOAD_PID" 2>/dev/null \
        | awk 'NR>1 {print $9}' | sed 's/.*://' | head -1)
    if [ -n "$samply_port" ]; then
        open -a Firefox "http://127.0.0.1:${samply_port}/from-url/http%3A%2F%2F127.0.0.1%3A${samply_port}%2Fprofile.json"
    else
        # Fallback: default port 3000+
        open -a Firefox "http://127.0.0.1:3000/from-url/http%3A%2F%2F127.0.0.1%3A3000%2Fprofile.json"
    fi

    echo -e "Artifacts in: ${BOLD}$WORK_DIR${RESET}"
    echo -e "Re-open later: ${DIM}samply load $profile_file${RESET}"
}

main "$@"
