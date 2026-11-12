#!/usr/bin/env python3
"""Verify the default allocator profile in a native Nera program.

The native process prints its own jemalloc opt.* readings as JSON at exit.
This catches an unprefixed, discarded or incorrectly typed malloc_conf symbol,
and verifies that _RJEM_MALLOC_CONF still overrides the compiled defaults.
Run after rebuilding the release runtime and compiler:
    python3 scripts/test-allocator-config.py [--nera /absolute/path/to/nera]
"""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nera", type=Path, default=Path("target/release/nera"))
    parser.add_argument("--temp-parent", type=Path)
    args = parser.parse_args()
    if os.name == "nt":
        print("SKIP: Nera's compiled allocator profile is non-Windows only")
        return
    compiler = args.nera.resolve()
    env = dict(os.environ)
    for key in list(env):
        if key.endswith("MALLOC_CONF") or key.endswith("JEMALLOC_OVERRIDE"):
            env.pop(key)
    # Invalid options must fail initialization rather than only print a warning.
    env["_RJEM_MALLOC_CONF"] = "abort_conf:true"
    with tempfile.TemporaryDirectory(prefix="nera allocator config ", dir=args.temp_parent) as temporary:
        work = Path(temporary)
        source = work / "allocator_config.erl"
        source.write_text('''-module(allocator_config).
-export([start/0]).
start() -> io:format("allocator config ok~n").
''')
        binary = work / ("allocator-config.exe" if os.name == "nt" else "allocator-config")
        subprocess.run([compiler, "build", source, "-e", "allocator_config:start", "-o", binary],
                       cwd=work, env=env, check=True, timeout=120)
        defaults = {"tcache_max": 4096, "narenas": 2}
        if sys.platform == "darwin":
            defaults["retain"] = False
        elif sys.platform.startswith("linux"):
            # jemalloc retains by default on 64-bit Linux, but not 32-bit.
            defaults["retain"] = struct.calcsize("P") == 8
        override_retain = not defaults.get("retain", False)
        retain_text = str(override_retain).lower()
        cases = [
            ("defaults", "", defaults),
            ("user override", f"retain:{retain_text},tcache_max:8192,narenas:3",
             {"retain": override_retain, "tcache_max": 8192, "narenas": 3}),
        ]
        for label, override, expected in cases:
            run_env = dict(env)
            # The unprefixed variable must not override the prefixed allocator.
            run_env["MALLOC_CONF"] = f"retain:{retain_text},tcache_max:16384,narenas:7"
            run_env["_RJEM_MALLOC_CONF"] = ",".join(filter(None, [
                "abort_conf:true,stats_print:true,stats_print_opts:J", override]))
            result = subprocess.run([binary], cwd=work, env=run_env, check=True,
                                    capture_output=True, text=True, timeout=30)
            assert result.stdout == "allocator config ok\n", result
            start = result.stderr.find('{"jemalloc"')
            assert start >= 0, result.stderr
            data, _ = json.JSONDecoder().raw_decode(result.stderr[start:])
            options = data["jemalloc"]["opt"]
            expected.update(dirty_decay_ms=10000, muzzy_decay_ms=0, tcache=True)
            for name, value in expected.items():
                assert options[name] == value, (label, name, options[name], value)
            # Mach-O does not support jemalloc background threads at this pin.
            if "background_thread" in options:
                assert options["background_thread"] is False, options
            print(f"{label}: {json.dumps({name: options[name] for name in expected}, sort_keys=True)}")


if __name__ == "__main__":
    main()
