#!/usr/bin/env python3
"""NERA-42: measure last stdout -> normal native exit, not startup/compilation.

Run on runtime changes after building runtime then compiler in release mode.
This is an opt-in performance probe: median <= 5 ms, not a maximum guarantee.
Every sample must produce complete expected output and exit without halt/kill.
"""
import argparse
import json
import math
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tempfile
import threading
import time


def measure(binary, env):
    with tempfile.TemporaryFile() as err:
        start = time.perf_counter_ns()
        process = subprocess.Popen([str(binary)], cwd=binary.parent, env=env,
                                   stdout=subprocess.PIPE, stderr=err)
        killed = threading.Event()

        def kill():
            killed.set()
            process.kill()

        watchdog = threading.Timer(5, kill)
        watchdog.start()
        chunks = []
        last = None
        try:
            while True:
                data = os.read(process.stdout.fileno(), 65536)
                observed = time.perf_counter_ns()
                if not data:
                    break
                chunks.append(data)
                last = observed
            code = process.wait()  # blocking wait, never timeout-polling
            exited = time.perf_counter_ns()
        finally:
            watchdog.cancel()
            watchdog.join()
            process.stdout.close()
        err.seek(0)
        stderr = err.read().decode(errors="replace")
        stdout = b"".join(chunks).decode()
        if killed.is_set() or code != 0 or last is None:
            raise RuntimeError((binary.name, code, killed.is_set(), stdout, stderr))
        return dict(gap_ms=(exited-last)/1e6, output_ms=(last-start)/1e6,
                    exit_ms=(exited-start)/1e6, stdout=stdout, stderr=stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nera", type=Path, default=Path("target/release/nera"))
    parser.add_argument("--runs", type=int, default=30)
    parser.add_argument("--budget-ms", type=float, default=5)
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
    results = dict(platform=platform.platform(), runs=args.runs,
                   revision=subprocess.check_output(["git", "rev-parse", "HEAD"],
                                                    cwd=root, text=True).strip(), cases=[])
    failed = False
    with tempfile.TemporaryDirectory(prefix="nera shutdown ", dir=args.temp_parent) as directory:
        work = Path(directory)
        shutil.copy(root / "examples/hello.erl", work)
        shutil.copy(root / "demo/serverless/handler.erl", work)
        fixtures = {
            "timer_exit": 'receive after 37 -> ok end, io:format("timer done~n")',
            "file_exit": '{ok, _} = file:read_file("hello.erl"), io:format("file done~n")',
            "child_exit": 'spawn(fun() -> receive after 37 -> io:format("child done~n") end end), ok',
        }
        for name, body in fixtures.items():
            (work / f"{name}.erl").write_text(
                f'-module({name}).\n-export([start/0]).\nstart() -> {body}.\n')
        expected = {
            "hello": "Hello, World!\n",
            "timer_exit": "timer done\n", "file_exit": "file done\n",
            "child_exit": "child done\n",
            # Full output of the existing packet demo, not a success-only marker.
            "handler": '#{ip => #{version => 4,ihl => 5,total_length => 60,ttl => 64,protocol => 6,src => {172,16,10,99},dst => {172,16,10,12}},transport => #{type => tcp,src_port => 49153,dst_port => 80,seq => 1,ack => 0,flags => #{syn => true,ack => false,fin => false,rst => false,psh => false,urg => false},window => 65535}}\n#{ip => #{version => 4,ihl => 5,total_length => 36,ttl => 128,protocol => 17,src => {10,0,0,5},dst => {8,8,8,8}},transport => #{type => udp,src_port => 12345,dst_port => 53,length => 16}}\n',
        }
        for name, output in expected.items():
            binary = work / (name + (".exe" if os.name == "nt" else ""))
            subprocess.run([compiler, "build", work / f"{name}.erl", "-e",
                            f"{name}:start", "-o", binary], cwd=work, env=env,
                           check=True, timeout=120)
            for schedulers in ["default", "1"]:
                run_env = dict(env)
                if schedulers != "default":
                    run_env["NERA_SCHEDULERS"] = schedulers
                rows = [measure(binary, run_env) for _ in range(args.runs)]
                for row in rows:
                    if row["stdout"] != output:
                        raise AssertionError((name, "incomplete/unexpected output", row))
                gaps = sorted(row["gap_ms"] for row in rows)
                summary = dict(name=name, schedulers=schedulers,
                               median_ms=statistics.median(gaps),
                               p95_ms=gaps[math.ceil(.95*len(gaps))-1], max_ms=max(gaps))
                summary["passed"] = summary["median_ms"] <= args.budget_ms
                failed |= not summary["passed"]
                results["cases"].append(dict(**summary, samples=rows))
                print(json.dumps(summary), flush=True)
    args.out.write_text(json.dumps(results, indent=2) + "\n")
    if failed:
        raise SystemExit("FAIL: native shutdown median exceeds budget")


if __name__ == "__main__":
    main()
