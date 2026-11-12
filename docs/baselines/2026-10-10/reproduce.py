#!/usr/bin/env python3
"""Compare the baseline's small correctness probes with BEAM.

Requires a built Nera compiler, erl and erlc. Writes only in a temporary
directory. Exit 1 means an observed behaviour differs from the expected result;
these probes intentionally fail on the recorded baseline.
"""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile


def run(command, cwd):
    try:
        result = subprocess.run(
            [str(arg) for arg in command], cwd=cwd, capture_output=True,
            text=True, timeout=30,
        )
        return {"exit": result.returncode, "stdout": result.stdout,
                "stderr": result.stderr}
    except subprocess.TimeoutExpired:
        return {"exit": 124, "stdout": "", "stderr": "timeout after 30 seconds"}


def beam(source, directory):
    directory.mkdir()
    path = directory / "probe.erl"
    # Erlang requires the filename to match the module declaration.
    module = source.split("-module(", 1)[1].split(")", 1)[0]
    path = path.with_name(module + ".erl")
    path.write_text(source)
    result = run(["erlc", path.name], directory)
    if result["exit"]:
        return {"stage": "compile", **result}
    return run(["erl", "-noshell", "-pa", ".", "-s", module, "start",
                "-s", "init", "stop"], directory)


def main():
    here = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nera", type=Path,
                        default=here.parents[2] / "target/release/nera")
    args = parser.parse_args()
    nera = args.nera.resolve()
    results = {}
    good = True
    with tempfile.TemporaryDirectory(prefix="nera-baseline-probes-") as temporary:
        work = Path(temporary)
        for module in ["resume_exception_probe", "late_reply_probe"]:
            source = here / (module + ".erl")
            expected = "before\nafter\n" if module.startswith("resume") else "no_late_reply\n"
            reference = beam(source.read_text(), work / (module + "-beam"))
            executable = work / module
            compiled = run([nera, "build", source, "-e", module + ":start",
                            "-o", executable], work)
            native = run([executable], work) if compiled["exit"] == 0 else {
                "stage": "compile", **compiled,
            }
            passed = all(r["exit"] == 0 and r["stdout"] == expected
                         for r in [reference, native])
            results[module] = {"expected": expected, "beam": reference,
                               "native": native, "pass": passed}
            good = good and passed

        source = here / "formatter_guard_probe.erl"
        formatted = run([nera, "fmt", "--stdout", source], work)
        reference = beam(source.read_text(), work / "formatter-original")
        # Current CLI emits no stdout for unchanged files; preserve the source.
        first = formatted["stdout"] or source.read_text()
        after = beam(first, work / "formatter-formatted") if formatted["exit"] == 0 else {
            "stage": "format", **formatted,
        }
        passed = all(r["exit"] == 0 and r["stdout"] == "matched\n"
                     for r in [reference, after])
        results["formatter_guard_probe"] = {
            "original_beam": reference, "formatted_beam": after,
            "formatted_source": first, "pass": passed,
        }
        good = good and passed
    print(json.dumps(results, indent=2))
    return 0 if good else 1


if __name__ == "__main__":
    raise SystemExit(main())
