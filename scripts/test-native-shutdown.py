#!/usr/bin/env python3
"""NERA-42: measure last stdout -> normal native exit, not startup/compilation.

Run on runtime changes after building runtime then compiler in release mode.
This is an opt-in performance probe: median <= 5 ms, not a maximum guarantee.
Every sample must produce complete expected output and exit without halt/kill.
The packet demo's maps may list their keys in any order (Nera and BEAM order
atom keys by different atom tables); every key, value and line must be there.
"""
import argparse
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import statistics
import subprocess
import tempfile
import threading
import time

ATOM_OR_INTEGER = re.compile(r"(-?[0-9]+)|([a-z][A-Za-z0-9_@]*)")


def read_terms(text):
    """Read printed Erlang terms, one per line, as comparable values.

    Covers what the packet demo prints: maps, tuples, atoms and integers.
    A map becomes the set of its entries, so maps are equal whatever order
    their keys print in. Anything else, spacing included, must be exact.
    Raises ValueError on a missing newline, a cut line or unknown text."""
    if not text.endswith("\n"):
        raise ValueError("output does not end with a newline")
    return [_read_line(line) for line in text[:-1].split("\n")]


def _read_line(line):
    term, end = _read_term(line, 0)
    if end != len(line):
        raise ValueError(f"unexpected text at column {end}: {line[end:end+20]!r}")
    return term


def _read_term(s, i):
    if s.startswith("#{", i):
        entries, i = _read_sequence(s, i + 2, _read_entry)
        if len({key for key, _ in entries}) != len(entries):
            raise ValueError(f"map repeats a key before column {i}")
        return ("map", frozenset(entries)), i
    if s.startswith("{", i):
        items, i = _read_sequence(s, i + 1, _read_term)
        return ("tuple", tuple(items)), i
    match = ATOM_OR_INTEGER.match(s, i)
    if not match:
        raise ValueError(f"no term at column {i}: {s[i:i+20]!r}")
    integer, atom = match.groups()
    return (int(integer) if integer else ("atom", atom)), match.end()


def _read_entry(s, i):
    key, i = _read_term(s, i)
    if not s.startswith(" => ", i):
        raise ValueError(f"expected ' => ' at column {i}")
    value, i = _read_term(s, i + 4)
    return (key, value), i


def _read_sequence(s, i, read_item):
    """Items separated by ',' up to the closing '}' of a map or tuple."""
    items = []
    if s.startswith("}", i):
        return items, i + 1
    while True:
        item, i = read_item(s, i)
        items.append(item)
        if s.startswith(",", i):
            i += 1
        elif s.startswith("}", i):
            return items, i + 1
        else:
            raise ValueError(f"expected ',' or '}}' at column {i}")


# Full stdout of demo/serverless/handler.erl, map keys in source order.
HANDLER_OUTPUT = (
    "#{ip => #{version => 4,ihl => 5,total_length => 60,ttl => 64,protocol => 6,"
    "src => {172,16,10,99},dst => {172,16,10,12}},"
    "transport => #{type => tcp,src_port => 49153,dst_port => 80,seq => 1,ack => 0,"
    "flags => #{syn => true,ack => false,fin => false,rst => false,psh => false,"
    "urg => false},window => 65535}}\n"
    "#{ip => #{version => 4,ihl => 5,total_length => 36,ttl => 128,protocol => 17,"
    "src => {10,0,0,5},dst => {8,8,8,8}},"
    "transport => #{type => udp,src_port => 12345,dst_port => 53,length => 16}}\n")


def check_comparison():
    """Negative controls: only key order may differ from HANDLER_OUTPUT."""
    expected = read_terms(HANDLER_OUTPUT)
    reordered = HANDLER_OUTPUT.replace("ttl => 64,protocol => 6",
                                       "protocol => 6,ttl => 64")
    first_line = HANDLER_OUTPUT.split("\n")[0] + "\n"
    broken = [first_line, first_line + first_line, HANDLER_OUTPUT[:-1],
              HANDLER_OUTPUT[:-30] + "\n", HANDLER_OUTPUT + "\n",
              HANDLER_OUTPUT.replace("ttl => 64", "ttl => 65"),
              HANDLER_OUTPUT.replace("ihl => 5,", "ihl => 5,ihl => 5,"),
              HANDLER_OUTPUT.replace("length => 16", "length => 16,extra => 1"),
              HANDLER_OUTPUT.replace("ttl => 64", "ttl =>64")]
    if reordered == HANDLER_OUTPUT or not matches(reordered, expected):
        raise AssertionError("reordered map keys must still match")
    for text in broken:
        if text == HANDLER_OUTPUT or matches(text, expected):
            raise AssertionError(("comparison accepts broken output", text))


def matches(stdout, expected):
    """Exact text, or the same terms when expected was read with read_terms."""
    if isinstance(expected, str):
        return stdout == expected
    try:
        return read_terms(stdout) == expected
    except ValueError:
        return False


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
    check_comparison()
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
            "handler": read_terms(HANDLER_OUTPUT),
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
                    if not matches(row["stdout"], output):
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
