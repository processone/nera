#!/usr/bin/env python3
"""Keep native crash reports raising-first, as Erlang stacktrace lists are.

Run after rebuilding the runtime and compiler. The shared Erlang regression
supplies the real nested calls; this checks the uncaught report on stderr.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nera", type=Path, default=Path("target/release/nera"))
    parser.add_argument("--temp-parent", type=Path)
    args = parser.parse_args()
    compiler = args.nera.resolve()
    source = Path(__file__).resolve().parents[1] / "tests" / "stacktrace_order.erl"
    with tempfile.TemporaryDirectory(prefix="nera stacktrace order ", dir=args.temp_parent) as temporary:
        work = Path(temporary)
        binary = work / ("stacktrace-order.exe" if os.name == "nt" else "stacktrace-order")
        subprocess.run(
            [compiler, "build", source, "-e", "stacktrace_order:crash", "-o", binary],
            cwd=work, check=True, timeout=120,
        )
        result = subprocess.run(
            [binary], cwd=work, capture_output=True, text=True, timeout=30,
        )
        print(result.stderr, end="")
        assert result.returncode == 1, result
        assert "error:boom" in result.stderr, result.stderr
        assert "Stacktrace (most recent call first):" in result.stderr, result.stderr
        frames = re.findall(r"^  stacktrace_order:([^/]+)/\d+", result.stderr, re.MULTILINE)
        assert frames == ["inner", "middle", "outer", "crash"], frames
        print("uncaught stacktrace order ok")


if __name__ == "__main__":
    main()
