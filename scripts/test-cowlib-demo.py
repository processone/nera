#!/usr/bin/env python3
"""Check that examples/cowlib_demo behaves the same natively and on OTP.

Intent (NERA-87): the demo runs 85 Cowlib calls plus 6 semantic probes through
`cowlib_demo:start_canonical/0`, which sorts map entries and spells out atoms.
This script builds that entry point with Nera, runs the same sources on OTP
(rebar3 + erl), sorts both outputs and requires every line to match, except
the two known lines in KNOWN_DIFFERENCES:

  cookie.setcookie, http.merge_pseudo_headers  bytes follow atom-key map order

A known line that starts to match is reported but does not fail; any other
difference, a missing line or a raised exception fails. This is a correctness
check only: it measures no time (see benchmarks/cowlib_bench.sh).

Needs a release build (cargo build --release), erl, erlc, rebar3 and network
access to hex.pm. Exit 0 pass, 1 mismatch, 77 skipped (a tool is missing).

    scripts/test-cowlib-demo.py [--nera PATH] [--keep DIR]
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
DEMO = ROOT / "examples" / "cowlib_demo"
EXPECTED_LINES = 92  # 85 checks + 6 semantic probes + SUMMARY

KNOWN_DIFFERENCES = {
    "cookie.setcookie",
    "http.merge_pseudo_headers",
}


def by_name(output):
    """Map check name -> full line (the first word of a line is its name)."""
    lines = {}
    for line in sorted(output.splitlines()):
        name = line.split(" ", 1)[0]
        if name in lines:
            raise ValueError("duplicate line for " + name)
        lines[name] = line
    return lines


def compare(native, otp):
    """Return (unexpected, still_known, now_equal) check names."""
    n, o = by_name(native), by_name(otp)
    # SUMMARY lines carry no name; they hold the counts and must match too.
    differing = {k for k in n.keys() | o.keys() if n.get(k) != o.get(k)}
    return (sorted(differing - KNOWN_DIFFERENCES),
            sorted(differing & KNOWN_DIFFERENCES),
            sorted(KNOWN_DIFFERENCES - differing))


def run(cmd, cwd, **kw):
    return subprocess.run(cmd, cwd=cwd, check=True, capture_output=True,
                          text=True, **kw)


def build_and_run(nera, work):
    demo = work / "demo"
    shutil.copytree(DEMO, demo)
    run([nera, "deps", "get"], demo)
    run([nera, "build", "-e", "cowlib_demo:start_canonical", "-o", "canon"], demo)
    native = run([str(demo / "canon")], demo, timeout=120).stdout
    run(["rebar3", "compile"], demo)
    (demo / "ebin").mkdir()
    run(["erlc", "-o", "ebin", "src/cowlib_demo.erl"], demo)
    otp = run(["erl", "-noshell", "-pa", "ebin", "_build/default/lib/cowlib/ebin",
               "-eval", "cowlib_demo:start_canonical(), halt()."],
              demo, timeout=120).stdout
    return native, otp


class CowlibDemoParity(unittest.TestCase):
    nera = None
    keep = None

    @classmethod
    def setUpClass(cls):
        missing = [t for t in ("erl", "erlc", "rebar3") if not shutil.which(t)]
        if missing or not Path(cls.nera).exists():
            raise unittest.SkipTest(
                "needs erl, erlc, rebar3 and %s (cargo build --release); missing: %s"
                % (cls.nera, ", ".join(missing) or "nera"))
        if cls.keep:
            work = Path(cls.keep)
            work.mkdir(parents=True, exist_ok=True)
        else:
            work = Path(tempfile.mkdtemp(prefix="cowlib_demo_"))
            cls.addClassCleanup(shutil.rmtree, work, True)
        cls.native, cls.otp = build_and_run(cls.nera, work)

    def test_nothing_raised_and_all_lines_present(self):
        for label, out in (("native", self.native), ("otp", self.otp)):
            self.assertEqual(len(out.splitlines()), EXPECTED_LINES, label)
            self.assertNotIn(" EXC ", out, label)
            self.assertIn("SUMMARY 91 checks, 91 returned, 0 raised", out, label)

    def test_sorted_output_matches_otp_except_known_lines(self):
        unexpected, still_known, now_equal = compare(self.native, self.otp)
        self.assertEqual(unexpected, [], "lines differ from OTP")
        print("known differences still present: %s" % still_known)
        if now_equal:
            print("note: now identical, remove from KNOWN_DIFFERENCES: %s" % now_equal)

    def test_comparison_detects_a_changed_value(self):
        # Negative control on real output: change one value, expect detection.
        broken = self.native.replace("qs.build ok <<", "qs.build ok <<0,", 1)
        self.assertNotEqual(broken, self.native)
        unexpected, _, _ = compare(broken, self.otp)
        self.assertEqual(unexpected, ["qs.build"])

    def test_comparison_detects_a_missing_line(self):
        shortened = "\n".join(l for l in self.native.splitlines()
                              if not l.startswith("qs.build ")) + "\n"
        unexpected, _, _ = compare(shortened, self.otp)
        self.assertEqual(unexpected, ["qs.build"])


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--nera", default=os.environ.get(
        "NERA", str(ROOT / "target" / "release" / "nera")))
    ap.add_argument("--keep", help="build in this directory and keep it")
    args, rest = ap.parse_known_args()
    CowlibDemoParity.nera, CowlibDemoParity.keep = args.nera, args.keep
    res = unittest.main(argv=[sys.argv[0]] + rest, exit=False, verbosity=2).result
    if res.skipped and res.wasSuccessful() and not res.testsRun - len(res.skipped):
        sys.exit(77)
    sys.exit(0 if res.wasSuccessful() else 1)
