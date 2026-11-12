#!/usr/bin/env python3
"""Measure how long a native `receive after N` waits before its output appears.

Each fixture prints a marker line just before it starts waiting, then prints a
final line once the timeout has fired. The probe timestamps every stdout chunk
as it arrives and reports the delay from the marker to the final line, minus
the requested wait (the lateness). Startup, compilation and exit are excluded.

On macOS every case also runs at utility and background priority (through
taskpolicy), where the kernel may postpone ordinary timed waits by up to
75 ms and 100 ms (timer coalescing).

Run on runtime changes after building runtime then compiler in release mode.
This is an opt-in performance probe, not a maximum-latency guarantee: each
case's median lateness must be within --budget-ms and no sample may resume
more than --early-ms before N; p95, maximum and every sample are recorded.
On a busy machine, macOS may hold a background-priority thread for tens of
milliseconds, which shows in the tail rather than the median.
Every sample must produce complete expected output and exit normally.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import threading
import time

MARKER = "waiting\n"

# name: (start/0 body, expected stdout, total requested wait in ms)
FIXTURES = {
    "main_after": (
        'io:format("waiting~n"), receive after 37 -> ok end, io:format("timer done~n")',
        MARKER + "timer done\n", 37),
    "child_after": (
        'spawn(fun() -> io:format("waiting~n"), '
        'receive after 37 -> io:format("child done~n") end end), ok',
        MARKER + "child done\n", 37),
    # Back-to-back waits: each one registers a new timer right after the
    # previous one fired, while the timer thread is computing its next sleep.
    "loop_after": (
        'io:format("waiting~n"), '
        'lists:foreach(fun(_) -> receive after 10 -> ok end end, lists:seq(1, 5)), '
        'io:format("loop done~n")',
        MARKER + "loop done\n", 50),
}


# name: command prefix that starts the program at that priority
PRIORITIES = {"normal": []}
if platform.system() == "Darwin":
    PRIORITIES["utility"] = ["taskpolicy", "-c", "utility"]
    PRIORITIES["background"] = ["taskpolicy", "-b"]


def measure(binary, prefix, env, expected):
    """Run once; return the marker and final-output times in ms after launch."""
    with tempfile.TemporaryFile() as err:
        start = time.perf_counter_ns()
        process = subprocess.Popen(prefix + [str(binary)], cwd=binary.parent, env=env,
                                   stdout=subprocess.PIPE, stderr=err)
        killed = threading.Event()

        def kill():
            killed.set()
            process.kill()

        watchdog = threading.Timer(5, kill)
        watchdog.start()
        output = b""
        marker_at = None
        done_at = None
        try:
            while True:
                data = os.read(process.stdout.fileno(), 65536)
                observed = time.perf_counter_ns()
                if not data:
                    break
                output += data
                text = output.decode(errors="replace")
                if marker_at is None and text.startswith(MARKER):
                    marker_at = observed
                if done_at is None and text == expected:
                    done_at = observed
            code = process.wait()
        finally:
            watchdog.cancel()
            watchdog.join()
            process.stdout.close()
        err.seek(0)
        stderr = err.read().decode(errors="replace")
        stdout = output.decode(errors="replace")
        if killed.is_set() or code != 0 or stdout != expected:
            raise RuntimeError((binary.name, code, killed.is_set(), stdout, stderr))
        return dict(marker_ms=(marker_at - start) / 1e6,
                    output_ms=(done_at - start) / 1e6,
                    wait_ms=(done_at - marker_at) / 1e6)


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[math.ceil(fraction * len(ordered)) - 1]


def revision(root):
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    changed = subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True)
    return dict(revision=head, uncommitted=changed.splitlines())


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--nera", type=Path, default=Path("target/release/nera"))
    parser.add_argument("--runs", type=int, default=30)
    parser.add_argument("--budget-ms", type=float, default=5)
    parser.add_argument("--early-ms", type=float, default=2)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--temp-parent", type=Path)
    args = parser.parse_args()
    if args.runs < 30:
        parser.error("at least 30 runs are required")
    root = Path(__file__).resolve().parent.parent
    compiler = args.nera.resolve()
    env = dict(os.environ)
    env.pop("NERA_SCHED_STATS", None)
    env.pop("NERA_SCHEDULERS", None)
    results = dict(platform=platform.platform(), runs=args.runs, **revision(root),
                   compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),
                   budget_ms=args.budget_ms, early_ms=args.early_ms,
                   load_average=os.getloadavg() if hasattr(os, "getloadavg") else None,
                   cases=[])
    failed = False
    with tempfile.TemporaryDirectory(prefix="nera timer ", dir=args.temp_parent) as directory:
        work = Path(directory)
        for name, (body, expected, wait_ms) in FIXTURES.items():
            source = work / f"{name}.erl"
            source.write_text(f'-module({name}).\n-export([start/0]).\nstart() -> {body}.\n')
            binary = work / (name + (".exe" if os.name == "nt" else ""))
            subprocess.run([compiler, "build", source, "-e", f"{name}:start", "-o", binary],
                           cwd=work, env=env, check=True, timeout=120)
            for priority, prefix in PRIORITIES.items():
                for schedulers in ["default", "1"]:
                    run_env = dict(env)
                    if schedulers != "default":
                        run_env["NERA_SCHEDULERS"] = schedulers
                    rows = [measure(binary, prefix, run_env, expected) for _ in range(args.runs)]
                    late = [row["wait_ms"] - wait_ms for row in rows]
                    summary = dict(name=name, priority=priority, schedulers=schedulers,
                                   requested_ms=wait_ms,
                                   median_late_ms=statistics.median(late),
                                   p95_late_ms=percentile(late, .95),
                                   max_late_ms=max(late), min_late_ms=min(late),
                                   median_output_ms=statistics.median(r["output_ms"] for r in rows))
                    summary["passed"] = (summary["median_late_ms"] <= args.budget_ms
                                         and summary["min_late_ms"] >= -args.early_ms)
                    failed |= not summary["passed"]
                    results["cases"].append(dict(**summary, samples=rows))
                    print(json.dumps(summary), flush=True)
    args.out.write_text(json.dumps(results, indent=2) + "\n")
    if failed:
        raise SystemExit("FAIL: native receive timeout lateness exceeds budget")


if __name__ == "__main__":
    main()
