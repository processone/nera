#!/usr/bin/env python3
"""NERA-38: standalone compilers sharing a temp folder must link their own runtime.

Build two compilers with different embedded runtime archives (runtime first,
compiler second), retaining each compiler and its matching archive. Run:
    python3 scripts/test-embedded-runtime.py \
        --compiler-a /path/to/nera-a --runtime-a /path/to/runtime-a.a \
        --compiler-b /path/to/nera-b --runtime-b /path/to/runtime-b.a

Uses real native runtime archives, copies the compilers away from external
libraries, and compiles/runs outside the checkout. Checks both extraction
orders, reuse, and concurrent cold extractions, including two of each compiler.
Requires the platform C linker; run on the compilers' native platform.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("compiler-a", "runtime-a", "compiler-b", "runtime-b"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--temp-parent", type=Path)
    args = parser.parse_args()
    archives = [args.runtime_a.resolve(), args.runtime_b.resolve()]
    expected = [digest(path) for path in archives]
    assert expected[0] != expected[1], "two different runtime archives are required"

    with tempfile.TemporaryDirectory(prefix="nera embedded runtime ", dir=args.temp_parent) as temporary:
        root = Path(temporary).resolve()
        compilers = []
        for index, original in enumerate((args.compiler_a, args.compiler_b)):
            install = root / f"compiler-{index}" / "bin"
            install.mkdir(parents=True)
            compiler = install / ("nera.exe" if os.name == "nt" else "nera")
            shutil.copy2(original.resolve(), compiler)
            compilers.append(compiler)
        cache = root / "shared-cache"
        cache.mkdir(mode=0o700)
        env = dict(os.environ, TMPDIR=str(cache), TMP=str(cache), TEMP=str(cache))

        def compile_and_run(index, label):
            work = root / label
            work.mkdir()
            source = work / "runtime_cache.erl"
            source.write_text('''-module(runtime_cache).
-export([start/0]).
start() ->
    [3,2,1] = lists:reverse([1,2,3]),
    io:format("runtime cache ok~n").
''')
            executable = work / ("program.exe" if os.name == "nt" else "program")
            result = subprocess.run(
                [compilers[index], "build", source, "-e", "runtime_cache:start", "-o", executable, "-v"],
                cwd=work, env=env, capture_output=True, text=True, timeout=120)
            assert result.returncode == 0, (label, result.stdout, result.stderr)
            prefix = "Using embedded runtime library: "
            paths = [Path(line.removeprefix(prefix)).resolve()
                     for line in result.stderr.splitlines() if line.startswith(prefix)]
            assert len(paths) == 1, result.stderr
            extracted = paths[0]
            linked = [Path(line.removeprefix("Linking with runtime: ")).resolve()
                      for line in result.stderr.splitlines() if line.startswith("Linking with runtime: ")]
            assert linked == [extracted], result.stderr
            expected_parent = cache / f"nera-runtime-{os.geteuid()}" if os.name == "posix" else cache
            assert extracted.parent == expected_parent, ("not using the runtime cache", result.stderr)
            if os.name == "posix":
                assert extracted.parent.stat().st_uid == os.geteuid()
                assert stat.S_IMODE(extracted.parent.stat().st_mode) == 0o700
                assert stat.S_IMODE(extracted.stat().st_mode) == 0o600
            assert digest(extracted) == expected[index], (
                label, "linked a different runtime", extracted, expected[index], digest(extracted))
            run = subprocess.run([executable], cwd=work, env=env, check=True,
                                 capture_output=True, text=True, timeout=30)
            assert run.stdout == "runtime cache ok\n", run
            print(f"{label}: {run.stdout.strip()}; runtime sha256={expected[index]}", flush=True)
            return extracted

        for order in ((0, 1), (1, 0)):
            shutil.rmtree(cache)
            cache.mkdir(mode=0o700)
            paths = {}
            for index in order:
                paths[index] = compile_and_run(index, f"order-{order[0]}-{index}")
            assert paths[0] != paths[1], "different runtimes share one cache file"
            for index in order:
                before = paths[index].stat()
                reused = compile_and_run(index, f"reuse-{order[0]}-{index}")
                assert reused == paths[index]
                assert reused.stat().st_mtime_ns == before.st_mtime_ns, "cache was rewritten"

        shutil.rmtree(cache)
        cache.mkdir(mode=0o700)
        with ThreadPoolExecutor(max_workers=4) as executor:
            jobs = [executor.submit(compile_and_run, index, f"concurrent-{number}")
                    for number, index in enumerate((0, 0, 1, 1))]
            paths = [job.result() for job in jobs]
        assert paths[0] == paths[1] and paths[2] == paths[3] and paths[0] != paths[2]
        archive_directory = paths[0].parent
        if os.name == "posix":
            assert set(cache.iterdir()) == {archive_directory}, "extraction left temporary directories"
        assert set(archive_directory.iterdir()) == set(paths), "extraction left temporary files"
        print("Both orders, unchanged cache reuse, and concurrent cold extractions passed.", flush=True)


if __name__ == "__main__":
    main()
