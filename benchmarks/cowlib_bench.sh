#!/usr/bin/env bash
#
# On-demand benchmark: examples/cowlib_demo on Nera and on OTP.
# Not a correctness check and not a gate: it prints numbers and never fails
# on speed. Correctness is scripts/test-cowlib-demo.py.
#
# Reports:
#   1. whole-program wall time (startup included), median of RUNS runs
#   2. warm loop: one pass over the demo's checks in a single process
#      (1 s warm-up, 11 timed rounds), per runtime
#   3. peak memory footprint of the whole program (macOS /usr/bin/time -l)
#
# The HTTP/2 machine check is left out of the warm loop: each call starts
# 5 s timers and native Nera keeps one OS thread per pending timer (NERA-88).
#
# Usage:
#   ./benchmarks/cowlib_bench.sh [RUNS]      # default 20
#
# Prerequisites: cargo build --release; erl, erlc, rebar3, python3; hex.pm.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
NERA="${NERA:-$PROJECT_DIR/target/release/nera}"
RUNS="${1:-20}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/cowlib_bench.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

for tool in erl erlc rebar3 python3; do
    command -v "$tool" >/dev/null || { echo "Error: $tool not found in PATH"; exit 1; }
done
[ -x "$NERA" ] || { echo "Error: $NERA missing (cargo build --release)"; exit 1; }

cp -R "$PROJECT_DIR/examples/cowlib_demo" "$WORK/demo"
cd "$WORK/demo"

echo "Building (Nera and OTP)..." >&2
"$NERA" deps get >/dev/null
"$NERA" build -o demo_native >/dev/null 2>&1
cp "$SCRIPT_DIR/cowlib_bench.erl" src/
"$NERA" build -e cowlib_bench:start -o bench_native >/dev/null 2>&1
rebar3 compile >/dev/null
mkdir ebin
erlc -o ebin src/cowlib_demo.erl src/cowlib_bench.erl
ERL=(erl -noshell -pa ebin _build/default/lib/cowlib/ebin)

echo "nera vs OTP — examples/cowlib_demo"
echo "OTP $(erl -noshell -eval 'io:format("~s", [erlang:system_info(otp_release)]), halt().')," \
     "$(uname -sm), $RUNS runs, load average:$(uptime | sed 's/.*load averages*://')"
echo

python3 - "$RUNS" "$WORK/demo" <<'EOF'
import statistics, subprocess, sys, time
runs, demo = int(sys.argv[1]), sys.argv[2]
cmds = {
    "nera": ["./demo_native"],
    "otp": ["erl", "-noshell", "-pa", "ebin", "_build/default/lib/cowlib/ebin",
            "-eval", "cowlib_demo:start(), halt()."],
}
def once(c):
    t = time.perf_counter()
    subprocess.run(c, cwd=demo, stdout=subprocess.DEVNULL, check=True)
    return (time.perf_counter() - t) * 1000
for c in cmds.values():           # warm-up
    once(c)
times = {k: [] for k in cmds}
for _ in range(runs):             # interleaved to share machine noise
    for k, c in cmds.items():
        times[k].append(once(c))
print("Whole program (startup included), wall ms: median (min-max)")
for k, v in times.items():
    print(f"  {k:5s} {statistics.median(v):8.1f}  ({min(v):.1f}-{max(v):.1f})")
EOF
echo

python3 - "$WORK/demo" <<'EOF'
import re, statistics, subprocess, sys
demo = sys.argv[1]
def passes(cmd):
    out = subprocess.run(cmd, cwd=demo, capture_output=True, text=True,
                         check=True, timeout=300).stdout
    m = re.search(r"^SUITE n=\d+ ns_per_pass=\[([\d,]+)\]", out, re.M)
    return [int(x) / 1000 for x in m.group(1).split(",")]
runtimes = {
    "nera": ["./bench_native"],
    "otp": ["erl", "-noshell", "-pa", "ebin", "_build/default/lib/cowlib/ebin",
            "-eval", "cowlib_bench:start(), halt()."],
}
print("Warm loop, one pass over the checks, microseconds: median (min-max)")
res = {}
for k, c in runtimes.items():
    res[k] = passes(c)
    v = res[k]
    print(f"  {k:5s} {statistics.median(v):8.1f}  ({min(v):.1f}-{max(v):.1f})")
print(f"  ratio nera/otp {statistics.median(res['nera'])/statistics.median(res['otp']):.1f}x")
EOF
echo

if [ "$(uname)" = Darwin ]; then
    echo "Peak memory footprint of the whole program, MB (macOS):"
    for pair in "nera:./demo_native" "otp:${ERL[*]} -eval cowlib_demo:start(),halt()."; do
        name="${pair%%:*}"; cmd="${pair#*:}"
        { /usr/bin/time -l $cmd >/dev/null; } 2>&1 \
            | awk -v n="$name" '/peak memory footprint/ {printf "  %-5s %6.1f\n", n, $1 / 1048576}'
    done
fi
