#!/usr/bin/env python3
"""Keep native crash reports raising-first, as Erlang stacktrace lists are,
and at BEAM's default depth of 8 frames.

Run after rebuilding the runtime and compiler. The shared Erlang regressions
supply the real nested calls; this checks the uncaught report on stderr.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tempfile


def crash_frames(compiler, work, module):
    """Build and run Module:crash/0; return the module's frames from stderr."""
    source = Path(__file__).resolve().parents[1] / "tests" / f"{module}.erl"
    binary = work / (f"{module}.exe" if os.name == "nt" else module)
    subprocess.run(
        [compiler, "build", source, "-e", f"{module}:crash", "-o", binary],
        cwd=work, check=True, timeout=120,
    )
    result = subprocess.run(
        [binary], cwd=work, capture_output=True, text=True, timeout=30,
    )
    print(result.stderr, end="")
    assert result.returncode == 1, result
    assert "error:boom" in result.stderr, result.stderr
    assert "Stacktrace (most recent call first):" in result.stderr, result.stderr
    pattern = rf"^  {module}:([^/]+)/\d+"
    return re.findall(pattern, result.stderr, re.MULTILINE)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nera", type=Path, default=Path("target/release/nera"))
    parser.add_argument("--temp-parent", type=Path)
    args = parser.parse_args()
    compiler = args.nera.resolve()
    with tempfile.TemporaryDirectory(prefix="nera stacktrace order ", dir=args.temp_parent) as temporary:
        work = Path(temporary)
        frames = crash_frames(compiler, work, "stacktrace_order")
        assert frames == ["inner", "middle", "outer", "crash"], frames
        print("uncaught stacktrace order ok")
        # The report prints the captured trace: the eight innermost of the
        # thirteen calls in tests/stacktrace_depth.erl, as BEAM keeps them.
        frames = crash_frames(compiler, work, "stacktrace_depth")
        assert frames == [f"l{n}" for n in range(12, 4, -1)], frames
        print("uncaught stacktrace depth ok")


if __name__ == "__main__":
    main()
